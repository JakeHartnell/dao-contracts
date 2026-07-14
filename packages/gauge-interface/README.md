# Gauge interface

Shared, typed query protocol between DAO DAO's gauge orchestrator and adapter
contracts. The package defines paginated option discovery, option validation,
and selected-set execution-message responses without depending on a concrete
adapter implementation. `SampleGaugeMsgs.selected` is a global allocation:
option keys are unique and nonempty, shares are positive, and their sum may be
less than but never greater than one. Concrete adapters call the shared
validator before constructing messages, so malformed direct queries cannot
request more than the configured epoch budget.

See [the gauge architecture documentation](../../contracts/gauges/ARCHITECTURE.md)
for authority and security assumptions.
