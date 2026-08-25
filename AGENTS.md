# FLYT managed runtime downstream contract

Read `/home/ubuntu/AGENTS.md` first. This repository owns the platform-neutral
FLYT runtime extensions used to create, authenticate, reconcile, and delete
externally managed remote-GPU sessions. It does not own Nova, Neutron,
Kubernetes, or any other orchestrator integration.

- Keep orchestrator-specific vocabulary and APIs outside this repository.
- Model integrations as workload, tenant, attachment, client address, optional
  preferred node, generation, profile, and credential.
- Preserve compatibility with upstream FLYT data-plane protocols where
  possible and keep legacy persisted-field/config aliases during migration.
- The Cluster Manager must build and start without CUDA or a GPU node.
- Session create/delete and client authentication must be idempotent and fail
  closed. Never persist plaintext session credentials.
