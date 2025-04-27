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

#pragma once
#include <iostream>
#include <memory>
#include <mutex>
#include <set>
#include <nix/store-api.hh>
#include <nix/local-store.hh>
#include <nix/remote-store.hh>
#include <nix/uds-remote-store.hh>
#include <nix/hash.hh>
#include <nix/path.hh>
#include <nix/serialise.hh>
#include <nix/shared.hh>
#include <rust/cxx.h>

template<class T> using RVec = rust::Vec<T>;
template<class T> using RBox = rust::Box<T>;
template<class T> using RSlice = rust::Slice<T>;
using RString = rust::String;
using RStr = rust::Str;
using RBasePathSlice = RSlice<const unsigned char>;
using RHashSlice = RSlice<const unsigned char>;

struct AsyncWriteSender;

struct RustSink : nix::Sink
{
	RBox<AsyncWriteSender> sender;
public:
	RustSink(RBox<AsyncWriteSender> sender);
	void operator () (std::string_view data) override;
	void eof();
};

// Opaque wrapper for nix::ValidPathInfo
class CPathInfo {
	nix::ref<const nix::ValidPathInfo> pi;
public:
	CPathInfo(nix::ref<const nix::ValidPathInfo> pi);
	std::unique_ptr<std::vector<std::string>> sigs();
	std::unique_ptr<std::vector<std::string>> references();
};

class CNixStore {
	std::shared_ptr<nix::Store> store;
public:
	CNixStore();

	RString store_dir();
	std::unique_ptr<CPathInfo> query_path_info(RBasePathSlice base_name);
	std::unique_ptr<std::vector<std::string>> compute_fs_closure(
		RBasePathSlice base_name,
		bool flip_direction,
		bool include_outputs,
		bool include_derivers);
	void nar_from_path(RVec<unsigned char> base_name, RBox<AsyncWriteSender> sender);
};

std::unique_ptr<CNixStore> open_nix_store();

// Relies on our definitions
#include "nixcp/src/bindings/mod.rs.h"
