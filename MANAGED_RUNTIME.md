# flyt-managed-runtime

`flyt-managed-runtime` extends FLYT so an external infrastructure orchestrator
can manage remote-GPU session identity, authentication, lifecycle, and
reconciliation. The extension is deliberately independent of OpenStack,
Kubernetes, KubeVirt, and bare-metal provisioning APIs.

The integration boundary uses these terms:

| Runtime field | Meaning | OpenStack adapter mapping |
| --- | --- | --- |
| `workload_id` | Stable external workload identity | Nova instance UUID |
| `tenant_id` | Isolation and quota owner | Keystone project ID |
| `attachment_id` | Managed network attachment identity | Neutron port ID |
| `client_address` | Address observed on the FLYT service path | Port fixed IP |
| `preferred_node` | Optional execution-node affinity | Empty unless policy selects one |
| `generation` | Rejects stale clients after recreation | Adapter session generation |

The client presents `workload_id`, `generation`, and a session credential.
Cluster Manager also verifies the observed source address. Only the credential
hash is persisted. A session without an available GPU node remains
`PENDING_CAPACITY`; this permits control-plane development and testing on
GPU-free infrastructure without advertising usable GPU capacity.

An orchestrator must call `GET_CAPABILITIES` before creating a session and
require `managed-session-v1`. `whole-gpu-mps` is the normal profile for GPUs
without MIG support, including the RTX 3090 Ti; `mig` advertises support for a
single orchestrator-isolated MIG device.

Legacy MongoDB field names (`instance_uuid`, `project_id`, `port_id`, `vm_ip`,
`host_ip`) and the legacy `[openstack-session]` client stanza remain read
compatible for migration. New records and generated client configuration use
the generic managed-runtime terms.

The Cluster Manager MongoDB stanza accepts `auth-source` and defaults it to
`admin`, matching standard root-credential container initialization while the
managed session records remain in the configured application database.
