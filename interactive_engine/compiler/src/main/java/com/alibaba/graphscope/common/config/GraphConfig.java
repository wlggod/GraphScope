/*
 * Copyright 2020 Alibaba Group Holding Limited.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

package com.alibaba.graphscope.common.config;

public class GraphConfig {
    public static final Config<String> GRAPH_META_SCHEMA_URI =
            Config.stringConfig("graph.schema", ".");

    public static final Config<String> GRAPH_META_STATISTICS_URI =
            Config.stringConfig("graph.statistics", "");

    public static final Config<String> GRAPH_STORE = Config.stringConfig("graph.store", "exp");

    public static final Config<Long> GRAPH_META_SCHEMA_FETCH_INTERVAL_MS =
            Config.longConfig("graph.meta.schema.fetch.interval.ms", 1000);

    public static final Config<Long> GRAPH_META_STATISTICS_FETCH_INTERVAL_MS =
            Config.longConfig("graph.meta.statistics.fetch.interval.ms", 24 * 3600 * 1000l);

    public static final Config<Long> GRAPH_META_FETCH_TIMEOUT_MS =
            Config.longConfig("graph.meta.fetch.timeout.ms", 1000);

    // an intermediate solution to support foreign key, will be integrated into schema
    public static final Config<String> GRAPH_FOREIGN_KEY_URI =
            Config.stringConfig("graph.foreign.key", "");

    public static final Config<String> GRAPH_FUNCTIONS_URI =
            Config.stringConfig("graph.functions", "");
}
