//
//! Copyright 2020 Alibaba Group Holding Limited.
//!
//! Licensed under the Apache License, Version 2.0 (the "License");
//! you may not use this file except in compliance with the License.
//! You may obtain a copy of the License at
//!
//! http://www.apache.org/licenses/LICENSE-2.0
//!
//! Unless required by applicable law or agreed to in writing, software
//! distributed under the License is distributed on an "AS IS" BASIS,
//! WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//! See the License for the specific language governing permissions and
//! limitations under the License.

use std::any::TypeId;
use std::fmt::Debug;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use opentelemetry::global::BoxedSpan;
use opentelemetry::{trace, trace::Span, KeyValue};
use pegasus_executor::{Task, TaskState};

use crate::api::primitive::source::Source;
use crate::channel_id::ChannelId;
use crate::communication::output::{OutputBuilder, OutputBuilderImpl};
use crate::data_plane::Push;
use crate::dataflow::{Dataflow, DataflowBuilder};
use crate::errors::{BuildJobError, JobExecError};
use crate::event::emitter::EventEmitter;
use crate::event::Event;
use crate::graph::Port;
use crate::progress::DynPeers;
use crate::progress::EndOfScope;
use crate::resource::{KeyedResources, ResourceMap};
use crate::result::ResultSink;
use crate::schedule::Schedule;
use crate::{Data, JobConf, Tag, WorkerId};

pub struct Worker<D: Data, T: Debug + Send + 'static> {
    pub conf: Arc<JobConf>,
    pub id: WorkerId,
    task: WorkerTask,
    peer_guard: Arc<AtomicUsize>,
    start: Instant,
    sink: ResultSink<T>,
    resources: ResourceMap,
    keyed_resources: KeyedResources,
    is_finished: bool,
    span: BoxedSpan,
    _ph: std::marker::PhantomData<D>,
}

impl<D: Data, T: Debug + Send + 'static> Worker<D, T> {
    pub(crate) fn new(
        conf: &Arc<JobConf>, id: WorkerId, peer_guard: &Arc<AtomicUsize>, sink: ResultSink<T>,
        span: BoxedSpan,
    ) -> Self {
        if peer_guard.fetch_add(1, Ordering::SeqCst) == 0 {
            pegasus_memory::alloc::new_task(conf.job_id as usize);
        }
        Worker {
            conf: conf.clone(),
            id,
            task: WorkerTask::Empty,
            peer_guard: peer_guard.clone(),
            start: Instant::now(),
            sink,
            resources: ResourceMap::default(),
            keyed_resources: KeyedResources::default(),
            is_finished: false,
            span: span,
            _ph: std::marker::PhantomData,
        }
    }

    pub fn dataflow<F>(&mut self, func: F) -> Result<(), BuildJobError>
    where
        F: FnOnce(&mut Source<D>, ResultSink<T>) -> Result<(), BuildJobError>,
    {
        // set current worker's id into tls variable to make it accessible at anywhere;
        let _g = crate::worker_id::guard(self.id);
        let resource = crate::communication::build_channel::<Event>(
            ChannelId::new(self.id.job_id, 0),
            &self.conf,
            self.id,
        )?;
        if resource.ch_id.index != 0 {
            return Err(BuildJobError::InternalError(String::from("Event channel index must be 0")));
        }
        let (mut tx, rx) = resource.take();
        if self.conf.total_workers() > 1 {
            if tx.len() != self.id.total_peers() as usize + 1 {
                return Err(BuildJobError::InternalError(format!(
                    "Incorrect number of senders, senders size: {}, total_peers: {};",
                    tx.len(),
                    self.id.total_peers(),
                )));
            }
            let mut abort = tx.swap_remove(self.id.index as usize);
            abort.close().ok();
        }
        let event_emitter = EventEmitter::new(tx);
        let dfb = DataflowBuilder::new(self.id, event_emitter.clone(), &self.conf);
        let root_builder = OutputBuilderImpl::new(
            Port::new(0, 0),
            0,
            self.conf.batch_size as usize,
            self.conf.batch_capacity,
        );
        let mut input = Source::new(root_builder.copy_data(), &dfb);
        let output = self.sink.clone();
        func(&mut input, output)?;
        let mut sch = Schedule::new(event_emitter, rx);
        let df = dfb.build(&mut sch)?;
        self.task = WorkerTask::Dataflow(df, sch);
        let root = Box::new(root_builder)
            .build()
            .expect("no output;");
        let end = EndOfScope::new(Tag::Root, DynPeers::all(self.id.total_peers()), 0, 0);
        root.notify_end(end).ok();
        root.close().ok();
        Ok(())
    }

    pub fn add_resource<R: Send + 'static>(&mut self, resource: R) {
        let type_id = TypeId::of::<R>();
        self.resources
            .insert(type_id, Box::new(resource));
    }

    pub fn add_resource_with_key<R: Send + 'static>(&mut self, key: String, resource: R) {
        self.keyed_resources
            .insert(key, Box::new(resource));
    }

    fn check_cancel(&self) -> bool {
        if self.conf.time_limit > 0 {
            let elapsed = self.start.elapsed().as_millis() as u64;
            if elapsed >= self.conf.time_limit {
                return true;
            }
        }
        self.sink
            .get_cancel_hook()
            .load(Ordering::SeqCst)
    }

    fn release(&mut self) {
        if self.peer_guard.load(Ordering::SeqCst) == 0 {
            pegasus_memory::alloc::remove_task(self.conf.job_id as usize);
        }
        if !crate::remove_cancel_hook(self.conf.job_id).is_ok() {
            error!("JOB_CANCEL_MAP is poisoned!");
        }
    }
}

enum WorkerTask {
    Empty,
    Dataflow(Dataflow, Schedule),
}

impl WorkerTask {
    pub fn execute(&mut self) -> Result<TaskState, JobExecError> {
        match self {
            WorkerTask::Empty => Ok(TaskState::Finished),
            WorkerTask::Dataflow(df, sch) => {
                sch.step(df)?;
                if df.check_finish() {
                    sch.close()?;
                    Ok(TaskState::Finished)
                } else if df.is_idle()? {
                    Ok(TaskState::NotReady)
                } else {
                    Ok(TaskState::Ready)
                }
            }
        }
    }

    pub fn check_ready(&mut self) -> Result<TaskState, JobExecError> {
        match self {
            WorkerTask::Empty => Ok(TaskState::Finished),
            WorkerTask::Dataflow(df, sch) => {
                sch.try_notify()?;
                if df.is_idle()? {
                    Ok(TaskState::NotReady)
                } else {
                    Ok(TaskState::Ready)
                }
            }
        }
    }
}

struct WorkerContext<'a> {
    resource: Option<&'a mut ResourceMap>,
    keyed_resources: Option<&'a mut KeyedResources>,
}

impl<'a> WorkerContext<'a> {
    fn new(res: &'a mut ResourceMap, key_res: &'a mut KeyedResources) -> Self {
        let resource = if !res.is_empty() {
            let reset = std::mem::replace(res, Default::default());
            let pre = crate::resource::replace_resource(reset);
            assert!(pre.is_empty());
            Some(res)
        } else {
            None
        };

        let keyed_resources = if !key_res.is_empty() {
            let reset = std::mem::replace(key_res, Default::default());
            let pre = crate::resource::replace_keyed_resources(reset);
            assert!(pre.is_empty());
            Some(key_res)
        } else {
            None
        };
        WorkerContext { resource, keyed_resources }
    }
}

impl<'a> Drop for WorkerContext<'a> {
    fn drop(&mut self) {
        if let Some(res) = self.resource.take() {
            let my_res = crate::resource::replace_resource(Default::default());
            let _r = std::mem::replace(res, my_res);
        }
        if let Some(res) = self.keyed_resources.take() {
            let my_res = crate::resource::replace_keyed_resources(Default::default());
            let _r = std::mem::replace(res, my_res);
        }
    }
}

impl<D: Data, T: Debug + Send + 'static> Task for Worker<D, T> {
    fn execute(&mut self) -> TaskState {
        let _g = crate::worker_id::guard(self.id);
        if self.check_cancel() {
            self.span
                .set_status(trace::Status::error("Job is canceled"));
            self.span.end();

            self.sink.set_cancel_hook(true);
            return TaskState::Finished;
        }

        let _ctx = WorkerContext::new(&mut self.resources, &mut self.keyed_resources);
        let trace_id = self.span.span_context().trace_id();
        let trace_id_hex = format!("{:x}", trace_id);

        match self.task.execute() {
            Ok(state) => {
                if TaskState::Finished == state {
                    let elapsed = self.start.elapsed().as_millis();
                    info_worker!(
                        "trace_id:{}, job({}) '{}' finished, used {:?} ms;",
                        trace_id_hex,
                        self.id.job_id,
                        self.conf.job_name,
                        elapsed
                    );
                    self.is_finished = true;
                    self.span
                        .set_attribute(KeyValue::new("used_ms", elapsed.to_string()));
                    self.span.set_status(trace::Status::Ok);
                    self.span.end();
                    // if this is last worker, return Finished
                    if self.peer_guard.fetch_sub(1, Ordering::SeqCst) == 1 {
                        state
                    } else {
                        // if other workers are not finished, return NotReady until all workers finished
                        TaskState::NotReady
                    }
                } else {
                    state
                }
            }
            Err(e) => {
                error_worker!("trace_id:{}, job({}) execute error: {}", trace_id_hex, self.id.job_id, e);
                self.span
                    .set_status(trace::Status::error(format!("Execution error: {}", e)));
                self.span.end();
                self.sink.on_error(e);
                TaskState::Finished
            }
        }
    }

    fn check_ready(&mut self) -> TaskState {
        let _g = crate::worker_id::guard(self.id);
        if self.is_finished && self.peer_guard.load(Ordering::SeqCst) == 0 {
            return TaskState::Finished;
        }
        if self.check_cancel() {
            self.sink.set_cancel_hook(true);
            return TaskState::Finished;
        }
        if !self.is_finished {
            match self.task.check_ready() {
                Ok(state) => {
                    {
                        if TaskState::Finished == state {
                            let elapsed = self.start.elapsed().as_millis();
                            info_worker!(
                                "job({}) '{}' finished, used {:?};",
                                self.id.job_id,
                                self.conf.job_name,
                                elapsed
                            );
                        }
                    }
                    state
                }
                Err(e) => {
                    error_worker!("job({}) execute error: {}", self.id.job_id, e);
                    self.sink.on_error(e);
                    TaskState::Finished
                }
            }
        } else {
            // all workers are finished, return state Finished
            if self.peer_guard.load(Ordering::SeqCst) == 0 {
                return TaskState::Finished;
            } else {
                return TaskState::NotReady;
            }
        }
    }
}

impl<D: Data, T: Debug + Send + 'static> Drop for Worker<D, T> {
    fn drop(&mut self) {
        self.release();
    }
}
