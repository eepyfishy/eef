# Local ambient-input consent

The node backend owns a local consent setting, a durable revision and an in-memory
revocation epoch. The command API exposes the same operation independently of the
browser:

```text
eefn listening show
eefn listening enable
eefn listening disable
```

Commands find the running node's local API automatically. They inspect its exact
node/runtime identity and current revision/epoch before sending one mutation.
Lost or malformed replies remain unconfirmed; commands do not retry, fall back to
another node, edit JSON offline, restart a service or start a capture workload.

Consent defaults off. Enabling it does not grant microphone permission, choose a
resource, install a model or authorize a job. The report states that product
capture integration is unavailable. Existing typed commands and connectivity
continue to work while ambient consent is disabled. Neither the API nor node
startup launches a microphone source.

Only the dedicated local-owner command can change a saved denial into consent.
Remote management approval does not grant that power. Generic configuration
saves and restores preserve backend-owned revisions and cannot revive a previous
enabled setting after a denial. This also keeps older configuration replacements
from silently rolling back the consent revision.

Disabling revokes the runtime epoch before persistence. If backup/disk writing
fails, runtime revocation remains in place even though the durable setting may
still be enabled. Inspect the current result before making another decision.
A fresh explicit local enable can clear that latch. An older enable request
cannot clear it, because it carries the prior runtime epoch. An off/on cycle
never makes an old supervised operation eligible again.

The trusted-library `NodeService::supervise_listening` wrapper requires saved
consent, both saved/applied microphone permission and an absolute caller deadline.
It drops its owned future when the consent epoch is revoked or the deadline
expires. A product caller must separately provide validated durable workload
admission, an exact resource and ongoing permission/runtime checks. This wrapper
does not create job authority or monitor arbitrary external configuration-file
edits. It is not a network execution entrypoint or a general termination proof.

Product continuous-workload integration remains open: separately scoped grants,
the existing job/attempt/receipt lifecycle, resource and provider admission,
detectors, retained-payload bounds, event deduplication/backpressure and coordinator
reporting. Real microphone/driver release and second-PC acceptance are also open.

Focused Rust checks cover default-off migration, restore/revision guards, local
ownership, forged reports, revocation and failed-save behavior. The synthetic
native fixture uses the real CLI and local API to disable a supervised Python
producer. Independent creation-verified Windows handles observe the producer and
its child exit; a lost mutation reply must cause exactly one write. This uses no
microphone hardware and supplies no production workload authority.
