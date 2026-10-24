# Alias declaration context

Audience: language users and library maintainers.

Transparent aliases now retain imported type identities from the file declaring
the alias. Imports at a use site cannot reinterpret those stored identities.
Public implicit prelude exports remain available, including a runtime owner
exported as `String` under a different original declaration name.

Rebuild source caches with the updated compiler (QZC30). QZI14's wire format is
unchanged; existing artifacts remain Unverified and should be rebuilt from source
to acquire current source checks. No import syntax changes are required.

Private namespace visibility across source files is still incomplete. Import
names explicitly in the file using them; successful checking alone does not
establish full module isolation. See [the support boundary](../internals/source-type-context.md).
