/*
Copyright 2022 Zhaofeng Li and the Attic contributors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

// C++ side of the libnixstore glue.
//
// We implement a mid-level wrapper of the Nix Store interface,
// which is then wrapped again in the Rust side to enable full
// async-await operation.
//
// Here we stick with the naming conventions of Rust and handle
// Rust types directly where possible, so that the interfaces are
// satisfying to use from the Rust side via cxx.rs.

#include "nixcp/src/bindings/nix.hpp"

static std::mutex g_init_nix_mutex;
static bool g_init_nix_done = false;

static nix::StorePath store_path_from_rust(RBasePathSlice base_name) {
	std::string_view sv((const char *)base_name.data(), base_name.size());
	return nix::StorePath(sv);
}

// ========
// RustSink
// ========

RustSink::RustSink(RBox<AsyncWriteSender> sender) : sender(std::move(sender)) {}

void RustSink::operator () (std::string_view data) {
	RBasePathSlice s((const unsigned char *)data.data(), data.size());

	this->sender->send(s);
}

void RustSink::eof() {
	this->sender->eof();
}


// =========
// CPathInfo
// =========

CPathInfo::CPathInfo(nix::ref<const nix::ValidPathInfo> pi) : pi(pi) {}

std::unique_ptr<std::vector<std::string>> CPathInfo::sigs() {
	std::vector<std::string> result;
	for (auto&& elem : this->pi->sigs) {
		result.push_back(std::string(elem));
	}
	return std::make_unique<std::vector<std::string>>(result);
}

std::unique_ptr<std::vector<std::string>> CPathInfo::references() {
	std::vector<std::string> result;
	for (auto&& elem : this->pi->references) {
		result.push_back(std::string(elem.to_string()));
	}
	return std::make_unique<std::vector<std::string>>(result);
}

// =========
// CNixStore
// =========

CNixStore::CNixStore() {
	std::map<std::string, std::string> params;
	std::lock_guard<std::mutex> lock(g_init_nix_mutex);

	if (!g_init_nix_done) {
		nix::initNix();
		g_init_nix_done = true;
	}

	this->store = nix::openStore(nix::settings.storeUri.get(), params);
}

std::unique_ptr<CPathInfo> CNixStore::query_path_info(RBasePathSlice base_name) {
	auto store_path = store_path_from_rust(base_name);

	auto r = this->store->queryPathInfo(store_path);
	return std::make_unique<CPathInfo>(r);
}

std::unique_ptr<std::vector<std::string>> CNixStore::compute_fs_closure(RBasePathSlice base_name, bool flip_direction, bool include_outputs, bool include_derivers) {
	std::set<nix::StorePath> out;

	this->store->computeFSClosure(store_path_from_rust(base_name), out, flip_direction, include_outputs, include_derivers);

	std::vector<std::string> result;
	for (auto&& elem : out) {
		result.push_back(std::string(elem.to_string()));
	}
	return std::make_unique<std::vector<std::string>>(result);
}

std::unique_ptr<CNixStore> open_nix_store() {
	return std::make_unique<CNixStore>();
}
