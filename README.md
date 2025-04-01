Runs `nix copy` under the hood but only uploads paths that don't exist in upstream caches. It's async so may also be somewhat faster. Unlike `nix copy`, we also upload build dependencies. You may also pass the `--recursive` flag to absolutely not miss anything (be warned though, it queues up a lot of paths to check against upstream caches (also idk why you'd ever want to use this honestly)). Specify upstream caches to check against with `--upstream-cache` (can be specified multiple times, `cache.nixos.org` is always included).

```
Usage: nixcp [OPTIONS] --to <BINARY CACHE> <PACKAGE>

Arguments:
  <PACKAGE>  Package to upload to the binary cache

Options:
      --to <BINARY CACHE>
          Address of the binary cache (passed to nix copy --to)
  -u, --upstream-cache <UPSTREAM_CACHE>
          Upstream cache to check against. Can be specified multiple times. cache.nixos.org is always included
  -r, --recursive
          Whether to pass --recursive to nix path-info. Can queue a huge number of paths to upload
      --upstream-checker-concurrency <UPSTREAM_CHECKER_CONCURRENCY>
          Concurrent upstream cache checkers [default: 32]
      --uploader-concurrency <UPLOADER_CONCURRENCY>
          Concurrent uploaders [default: 16]
      --nix-store-concurrency <NIX_STORE_CONCURRENCY>
          Concurrent nix-store commands to run [default: 32]
  -h, --help
          Print help
  -V, --version
          Print version
```