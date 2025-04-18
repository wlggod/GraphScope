/** Copyright 2020 Alibaba Group Holding Limited.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * 	http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#include <csignal>
#include <filesystem>
#include <iostream>

#include <glog/logging.h>

#include <boost/program_options.hpp>

#include "flex/engines/graph_db/database/graph_db.h"
#include "flex/engines/http_server/options.h"

#ifdef BUILD_WITH_OSS
#include <boost/process.hpp>
#include "flex/utils/remote/oss_storage.h"
#endif

namespace bpo = boost::program_options;

static std::string work_dir;

void signal_handler(int signal) {
  LOG(INFO) << "Received signal " << signal << ", exiting...";
  // support SIGKILL, SIGINT, SIGTERM
  if (signal == SIGKILL || signal == SIGINT || signal == SIGTERM ||
      signal == SIGSEGV || signal == SIGABRT) {
    LOG(ERROR) << "Received signal " << signal
               << ",Clearing directory: " << work_dir << ", exiting...";
    // remove all files in work_dir
    std::filesystem::remove_all(work_dir);
    exit(signal);
  } else {
    LOG(ERROR) << "Received unexpected signal " << signal << ", exiting...";
    exit(1);
  }
}

#ifdef BUILD_WITH_OSS

void check_oss_object_not_exist(std::string& data_path,
                                std::string& object_path,
                                gs::OSSConf& oss_conf) {
  auto pos = data_path.find("/", 6);
  if (pos == std::string::npos) {
    LOG(FATAL) << "Invalid data path: " << data_path;
  }
  oss_conf.bucket_name_ = data_path.substr(6, pos - 6);
  object_path = data_path.substr(pos + 1);
  oss_conf.load_conf_from_env();
  // check whether the object exists
  auto oss_reader = std::make_shared<gs::OSSRemoteStorageDownloader>(oss_conf);
  if (!oss_reader || !oss_reader->Open().ok()) {
    LOG(FATAL) << "Failed to open oss reader";
  }
  std::vector<std::string> path_list;
  auto status = oss_reader->List(object_path, path_list);
  if (status.ok() && path_list.size() > 0) {
    LOG(FATAL) << "Object already exists: " << object_path
               << ", list size: " << path_list.size()
               << ", please remove the object and try again.";
  }
  // use a random directory
  data_path = "/tmp/" + std::to_string(time(nullptr));
}

int32_t upload_data_dir_to_oss(const std::filesystem::path& data_dir_path,
                               const std::string& object_path,
                               const gs::OSSConf& oss_conf) {
  // zip the data directory
  std::string zip_file = data_dir_path.string() + ".zip";
  std::string zip_cmd = "zip -r " + zip_file + " " + data_dir_path.string();
  boost::process::child zip_process(zip_cmd);
  zip_process.wait();

  int res = zip_process.exit_code();
  if (res != 0) {
    LOG(ERROR) << "Failed to zip data directory: " << zip_cmd
               << ", code: " << res;
    return -1;
  }

  auto oss_writer = std::make_shared<gs::OSSRemoteStorageUploader>(oss_conf);
  if (!oss_writer || !oss_writer->Open().ok()) {
    LOG(ERROR) << "Failed to open oss writer";
    return -1;
  }
  auto status = oss_writer->Put(zip_file, object_path, false);
  if (!status.ok()) {
    LOG(ERROR) << "Failed to upload data to oss: " << status.ToString();
    return -1;
  }
  status = oss_writer->Close();
  if (!status.ok()) {
    LOG(ERROR) << "Failed to close oss writer: " << status.ToString();
    return -1;
  }
  LOG(INFO) << "Successfully uploaded data to oss: " << object_path
            << ", it is in zip format";
  std::filesystem::remove(zip_file);
  std::filesystem::remove_all(data_dir_path);
  return 0;
}
#endif

int main(int argc, char** argv) {
  bpo::options_description desc("Usage:");
  /**
   * When loading the edges of a graph, there are two stages involved.
   *
   * The first stage involves reading the edges into a temporary vector and
   * acquiring information on the degrees of the vertices,
   * Then constructs the CSR using the degree information.
   *
   * During the first stage, the edges are stored in the form of triplets, which
   * can lead to a certain amount of memory expansion, so the `use-mmap-vector`
   * option is provided, mmap_vector utilizes mmap to map files, supporting
   * runtime memory swapping to disk.
   *
   * Constructing the CSR involves random reads and writes, we offer the
   * `build-csr-in-mem` option, which allows CSR to be built in-memory to
   * avoid extensive disk random read and write operations
   *
   */
  desc.add_options()("help", "Display help message")(
      "version,v", "Display version")("parallelism,p", bpo::value<uint32_t>(),
                                      "parallelism of bulk loader")(
      "data-path,d", bpo::value<std::string>(), "data directory path")(
      "graph-config,g", bpo::value<std::string>(), "graph schema config file")(
      "bulk-load,l", bpo::value<std::string>(), "bulk-load config file")(
      "build-csr-in-mem,m", bpo::value<bool>(), "build csr in memory")(
      "use-mmap-vector", bpo::value<bool>(), "use mmap vector");

  google::InitGoogleLogging(argv[0]);
  FLAGS_logtostderr = true;

  bpo::variables_map vm;
  bpo::store(bpo::command_line_parser(argc, argv).options(desc).run(), vm);
  bpo::notify(vm);

  if (vm.count("help")) {
    std::cout << desc << std::endl;
    return 0;
  }

  if (vm.count("version")) {
    std::cout << "GraphScope/Flex version " << FLEX_VERSION << std::endl;
    return 0;
  }

  std::string data_path = "";
  /**
   * If the data path is an oss path, the data will be uploaded to oss after
   * loading to a temporary directory. To improve the performance of the
   * performance, bulk_loader will zip the data directory before uploading.
   * The data path should be in the format of oss://bucket_name/object_path
   */
#ifdef BUILD_WITH_OSS
  bool upload_to_oss = false;
  std::string object_path = "";
  auto oss_conf = gs::OSSConf();
#endif
  std::string bulk_load_config_path = "";
  std::string graph_schema_path = "";

  if (!vm.count("graph-config")) {
    LOG(ERROR) << "graph-config is required";
    return -1;
  }
  graph_schema_path = vm["graph-config"].as<std::string>();
  if (!vm.count("data-path")) {
    LOG(ERROR) << "data-path is required";
    return -1;
  }
  data_path = vm["data-path"].as<std::string>();
  if (!vm.count("bulk-load")) {
    LOG(ERROR) << "bulk-load-config is required";
    return -1;
  }
  bulk_load_config_path = vm["bulk-load"].as<std::string>();

  setenv("TZ", "Asia/Shanghai", 1);
  tzset();

  double t = -grape::GetCurrentTime();

  auto schema_res = gs::Schema::LoadFromYaml(graph_schema_path);
  if (!schema_res.ok()) {
    LOG(ERROR) << "Fail to load graph schema file: "
               << schema_res.status().error_message();
    return -1;
  }
  auto loading_config_res = gs::LoadingConfig::ParseFromYamlFile(
      schema_res.value(), bulk_load_config_path);
  if (!loading_config_res.ok()) {
    LOG(ERROR) << "Fail to parse loading config file: "
               << loading_config_res.status().error_message();
    return -1;
  }

  // check whether parallelism, build_csr_in_mem, use_mmap_vector are overridden
  if (vm.count("parallelism")) {
    loading_config_res.value().SetParallelism(vm["parallelism"].as<uint32_t>());
  }
  if (vm.count("build-csr-in-mem")) {
    loading_config_res.value().SetBuildCsrInMem(
        vm["build-csr-in-mem"].as<bool>());
  }
  if (vm.count("use-mmap-vector")) {
    loading_config_res.value().SetUseMmapVector(
        vm["use-mmap-vector"].as<bool>());
  }

  if (data_path.find("oss://") == 0) {
#ifdef BUILD_WITH_OSS
    upload_to_oss = true;
    check_oss_object_not_exist(data_path, object_path, oss_conf);
#else
    LOG(ERROR) << "OSS is not supported in this build";
    return -1;
#endif
  }

  std::filesystem::path data_dir_path(data_path);
  if (!std::filesystem::exists(data_dir_path)) {
    std::filesystem::create_directory(data_dir_path);
  }
  std::filesystem::path serial_path = data_dir_path / "schema";
  if (std::filesystem::exists(serial_path)) {
    LOG(WARNING) << "data directory is not empty: " << data_dir_path.string()
                 << ", please remove the directory and try again.";
    return -1;
  }

  {
    std::error_code ec;
    std::filesystem::copy(graph_schema_path, data_dir_path / "graph.yaml",
                          std::filesystem::copy_options::overwrite_existing,
                          ec);
    if (ec) {
      LOG(FATAL) << "Failed to copy graph schema file: " << ec.message();
    }
  }

  work_dir = data_dir_path.string();

  // Register handlers for SIGKILL, SIGINT, SIGTERM, SIGSEGV, SIGABRT
  // LOG(FATAL) cause SIGABRT
  std::signal(SIGINT, signal_handler);
  std::signal(SIGTERM, signal_handler);
  std::signal(SIGKILL, signal_handler);
  std::signal(SIGSEGV, signal_handler);
  std::signal(SIGABRT, signal_handler);

  auto loader = gs::LoaderFactory::CreateFragmentLoader(
      data_dir_path.string(), schema_res.value(), loading_config_res.value());

  auto result = loader->LoadFragment();
  if (!result.ok()) {
    std::filesystem::remove_all(data_dir_path);
    LOG(ERROR) << "Failed to load fragment: "
               << result.status().error_message();
    return -1;
  }
  t += grape::GetCurrentTime();
  LOG(INFO) << "Finished bulk loading in " << t << " seconds.";

#ifdef BUILD_WITH_OSS
  if (upload_to_oss) {
    return upload_data_dir_to_oss(data_dir_path, object_path, oss_conf);
  }
#endif

  return 0;
}
