#!/usr/bin/env bash
set -Eeuo pipefail

: "${FLYT_NODE_MANAGER_CONFIG:?}"
manager_address=${FLYT_CLUSTER_MANAGER_ADDRESS:-}
expected_gpu_uuid=${FLYT_EXPECTED_GPU_UUID:-}
expected_product_name=${FLYT_EXPECTED_GPU_PRODUCT_NAME:-}
expected_device_count=${FLYT_EXPECTED_DEVICE_COUNT:-1}
mps_pipe=${CUDA_MPS_PIPE_DIRECTORY:-/run/flyt-mps/pipe}
mps_log=${CUDA_MPS_LOG_DIRECTORY:-/var/log/flyt-mps}

if [[ ! -s $FLYT_NODE_MANAGER_CONFIG ]]; then
  [[ $manager_address =~ ^[A-Za-z0-9._-]+:[0-9]{1,5}$ ]] || {
    echo "FLYT_CLUSTER_MANAGER_ADDRESS must be host:port" >&2
    exit 1
  }
  manager_host=${manager_address%:*}
  manager_port=${manager_address##*:}
  install -d "$(dirname "$FLYT_NODE_MANAGER_CONFIG")" /run/flyt
  cat >"$FLYT_NODE_MANAGER_CONFIG" <<EOF
[resource-manager]
address = "$manager_host"
port = $manager_port

[network]
advertise-address = "$manager_host"

[virt-server]
program-path = "/opt/flyt/bin/cricket-rpc-server"
thread-mode = 0
program-args = ""

[ipc]
mqueue-path = "/run/flyt/node-manager.queue"
EOF
  chmod 0600 "$FLYT_NODE_MANAGER_CONFIG"
fi

cleanup() {
  set +e
  [[ -n ${node_manager_pid:-} ]] && kill -TERM "$node_manager_pid" 2>/dev/null
  [[ -n ${node_manager_pid:-} ]] && wait "$node_manager_pid" 2>/dev/null
  pkill -TERM -P 1 cricket-rpc-server 2>/dev/null
  printf 'quit\n' | CUDA_MPS_PIPE_DIRECTORY="$mps_pipe" nvidia-cuda-mps-control 2>/dev/null
  [[ -n ${rpcbind_pid:-} ]] && kill -TERM "$rpcbind_pid" 2>/dev/null
}
trap cleanup EXIT
trap 'exit 0' INT TERM

gpu_inventory=$(nvidia-smi -L)
printf '%s\n' "$gpu_inventory"
if [[ -n $expected_gpu_uuid ]] && ! grep -Fq "$expected_gpu_uuid" <<<"$gpu_inventory"; then
  echo "expected GPU $expected_gpu_uuid is not visible" >&2
  exit 1
fi

visible_count=$(nvidia-smi --query-gpu=uuid --format=csv,noheader | sed '/^[[:space:]]*$/d' | wc -l)
[[ $visible_count -eq $expected_device_count ]] || {
  echo "expected $expected_device_count CUDA-visible device, observed $visible_count" >&2
  exit 1
}

if [[ -n $expected_product_name ]]; then
  visible_product_names=$(nvidia-smi --query-gpu=name --format=csv,noheader)
  [[ $visible_count -eq 1 && $visible_product_names == "$expected_product_name" ]] || {
    echo "expected GPU product '$expected_product_name', found '$visible_product_names'" >&2
    exit 1
  }
fi

install -d "$mps_pipe" "$mps_log" /run/rpcbind
if rpcinfo -p 127.0.0.1 >/dev/null 2>&1; then
  echo "using reachable network-namespace rpcbind"
else
  rpcbind -f -w &
  rpcbind_pid=$!
  sleep 1
  kill -0 "$rpcbind_pid"
fi

export CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES:-0}
export CUDA_MPS_PIPE_DIRECTORY="$mps_pipe"
export CUDA_MPS_LOG_DIRECTORY="$mps_log"
export CUDA_MPS_ENABLE_PER_CTX_DEVICE_MULTIPROCESSOR_PARTITIONING=1
nvidia-cuda-mps-control -d

/opt/flyt/bin/flyt-node-manager &
node_manager_pid=$!
wait "$node_manager_pid"
