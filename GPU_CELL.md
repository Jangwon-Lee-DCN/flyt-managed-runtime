# GPU Cell runtime

The GPU Cell packages one FLYT node manager and the Cricket RPC servers that it
creates around a single CUDA-visible GPU or MIG device. Kubernetes placement
and device allocation are external concerns; this repository does not create
ResourceClaims or Pods.

The initial implementation ports the GPU execution work from
`WoogiBoogi1129/flyt-k8s-poc` revision
`4b3290866dfe88d42d4cbd4011a9c5c2e60a3d0b` while preserving the
`flyt-managed-runtime` managed-session protocol. It includes:

- CUDA-visible device discovery suitable for CDI/DRA isolation;
- CUDA-derived MIG SM and memory capacity instead of parent-GPU NVML values;
- a NUL-safe `ftok(3)` client-manager queue path;
- logical CUDA device validation and stream creation fixes;
- cuDNN 9 build compatibility;
- recent CUDA driver-entry lookup and remote kernel attribute translation;
- an immutable GPU Cell image definition and MPS-aware entrypoint.

Build from the repository root so both `control-managers` and `cpu` are in the
context:

```bash
docker build -f control-managers/Dockerfile.gpu-cell .
```

The Pod must mount a node-manager TOML at `FLYT_NODE_MANAGER_CONFIG`, inject
exactly one CUDA device, and provide writable `/run/flyt-mps`,
`/var/log/flyt-mps`, `/run/rpcbind`, and `/dev/shm` volumes. Set
`FLYT_EXPECTED_GPU_UUID` when the orchestrator can provide a stable physical or
MIG UUID. The entrypoint fails closed if the expected device is not visible or
the visible-device count is not one.
For a device-specific product, set `FLYT_EXPECTED_GPU_PRODUCT_NAME` to the
exact `nvidia-smi --query-gpu=name` value. The entrypoint also fails before MPS
or Node Manager starts when the allocated device model differs; this is the
runtime check paired with the orchestrator's device selector.
`control-managers/gpu-cell-config.example.toml` documents the required manager
endpoint, RPC server, and IPC fields.

The source port deliberately does not claim full PyTorch compatibility yet.
Pinned-host-memory fallback and complete memory-quota accounting from the PoC
depend on its newer custom FLYT resource-map lineage and require a dedicated
port plus physical-GPU tests. Until those tests pass, production exposure and
Placement inventory publication remain prohibited.

The client package includes `flyt-cuda-smoke`, a dynamically linked CUDA
allocation/copy/kernel/copy-back probe plus its pinned CUDA runtime library.
It is an acceptance instrument for approved plain Ubuntu images, not a general
CUDA toolkit. A VM is data-plane accepted only when the probe prints
`FLYT_CUDA_SMOKE_OK result=42` through the intercepted remote path.

When a Cell uses host networking for a routable RPC endpoint, the entrypoint
reuses an already reachable rpcbind in that network namespace instead of
trying to bind a second port 111. Otherwise it starts and owns an isolated
rpcbind as before.
