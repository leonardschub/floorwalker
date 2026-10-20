# Allow account deletion to be reversed for 30 days

> Superseded by the [local-only spec](../../.scratch/local-only/spec.md) (2026-10-20): the app runs entirely on the user's machine and has no accounts, and the Worker that carried this out is removed. Kept as history.

Account deletion is self-service and includes a 30-day recovery period before permanent deletion. This gives the account holder time to reverse their decision, so the system must retain the account information needed for recovery during that period rather than erase it immediately.
