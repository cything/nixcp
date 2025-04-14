Uploads stuff to your s3 binary cache, but skip stuff that exist on upstream caches to save you space and time. Unlike `nix copy`, we also upload build dependencies meaning you just say the package or store path and we figure out the rest. Specify upstream caches to check against with `-u` (can be specified multiple times, `cache.nixos.org` is always included).

## Usage

Example:
```
nixcp --bucket nixcache --signing-key ~/cache-priv-key.pem --endpoint https://s3.cy7.sh -u https://nix-community.cachix.org push github:cything/nixcp/2025-04-12
```
The signing key is generated with:
```
nix-store --generate-binary-cache-key nixcache.cy7.sh cache-priv-key.pem cache-pub-key.pem
```

`AWS_ACCESS_KEY_ID` and `AWS_ENDPOINT_URL` environment variables should be set with your s3 credentials.

```
Usage: nixcp [OPTIONS] --bucket <bucket name> --signing-key <SIGNING_KEY> <COMMAND>

Commands:
  push  
  help  Print this message or the help of the given subcommand(s)

Options:
      --bucket <bucket name>
          The s3 bucket to upload to
  -u, --upstream <nixcache.example.com>
          Upstream cache to check against. Can be specified multiple times. cache.nixos.org is always included
      --signing-key <SIGNING_KEY>
          Path to the file containing signing key e.g. ~/cache-priv-key.pem
      --region <REGION>
          If unspecified, will get it form AWS_DEFAULT_REGION envar or the AWS default
      --endpoint <ENDPOINT>
          If unspecifed, will get it from AWS_ENDPOINT_URL envar or the AWS default e.g. https://s3.example.com
      --profile <PROFILE>
          AWS profile to use
  -h, --help
          Print help
  -V, --version
          Print version
```

## Install with nix
```
nix profile install github:cything/nixcp
```
Or run without installing:
```
nix run github:cything/nixcp
```
Separate arguments with `--` to pass them through to `nixcp` like so:
```
nix run github:cything/nixcp -- --help
```
