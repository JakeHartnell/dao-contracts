# Release evidence formats

The release manifest accepts two tracked JSON attestations and validates them
with `scripts/validate-gauge-release-evidence.sh`. `format_version` is the
integer `1`, and both documents must use the exact 40-character source commit
being released. Run `scripts/test-gauge-release-evidence.sh` for complete
minimal fixtures and rejection examples.

## Audit attestation

Use `kind: "gauge_security_audit_attestation"`. Record the independent
auditor, immutable report URI and SHA-256, and the exact three contract package
names. `findings.unresolved_critical` and `unresolved_high` must both be zero.
Auditor and maintainer sign-offs require names, dates, and `approved: true`.
Lower-severity accepted findings remain counted; their rationale belongs in
the immutable audit report.

## Chain test report

Use `kind: "gauge_chain_test_report"`. `deployments` must contain both `local`
and `public_testnet` records with chain/VM versions, evidence URI, and all three
contracts' package, checksum, code ID, address, and Wasm size. Checksums and
sizes must agree across networks and with the exact manifest artifacts.

Every scenario named in the validator must have passing evidence on both
networks. Every required worst-case operation needs gas used/limit, response
size, evidence, and a positive declared safety margin that the measurement
actually meets. The soak record requires at least two epochs, concurrent
keepers, conservative maximum state, and evidence. Finally, a maintainer must
approve the report by name and date.

These attestations do not replace the underlying signed audit, transaction
records, RPC exports, dashboard history, or governance decision. They bind and
summarize that evidence so the release gate is deterministic.

## Production approval

The release manifest is a pre-canary artifact and is not production deployment
authorization. After the canary, run:

```sh
scripts/validate-gauge-production-approval.sh \
  artifacts/gauges/manifest.json \
  path/to/canary-report.json \
  path/to/governance-approval.json
```

The canary report is hash-bound to the manifest and source commit. It records a
low-risk public-testnet gauge, at least two executed epochs, a power change, a
completed multi-call reset, consistent health samples, alert disposition,
immutable dashboard evidence, and staged proposed production limits. The
governance record is hash-bound to both earlier documents, records the canary
and production chain IDs separately, and must approve exactly those limits
after the canary report exists. Cross-chain block heights are deliberately not
compared. Any limit expansion requires a new canary recommendation and
governance record; editing an approved record breaks its hash binding.

`scripts/test-gauge-production-approval.sh` contains complete minimal fixtures
and adversarial rejection cases. Retain the three validated JSON documents
with the production proposal and deployment transaction evidence.
