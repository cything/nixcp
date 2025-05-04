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

`AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` environment variables should be set with your s3 credentials.

```
Usage: nixcp push [OPTIONS] --bucket <bucket name> --signing-key <SIGNING_KEY> [PATH]...

Arguments:
  [PATH]...  Path to upload e.g. ./result or /nix/store/y4qpcibkj767szhjb58i2sidmz8m24hb-hello-2.12.1

Options:
      --bucket <bucket name>
          The s3 bucket to upload to
  -u, --upstream <nixcache.example.com>
          Upstream cache to check against. Can be specified multiple times. cache.nixos.org is always included
      --signing-key <SIGNING_KEY>
          Path to the file containing signing key e.g. ~/cache-priv-key.pem
      --region <REGION>
          If unspecified, will get it form AWS_DEFAULT_REGION envar or default to us-east-1
      --endpoint <ENDPOINT>
          If unspecifed, will get it from AWS_ENDPOINT envar e.g. https://s3.example.com
      --skip-signature-check
          
  -h, --help
          Print help
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
