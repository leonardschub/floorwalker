# Credential changes revoke authentication via a credential epoch

> Superseded by the [local-only spec](../../.scratch/local-only/spec.md) (2026-10-20): the app runs entirely on the user's machine and has no accounts, and the Worker that carried this out is removed. Kept as history.

A password reset must consume its token exactly once and revoke every session, pending login challenge, and in-flight authentication established against the previous password. Deleting rows cannot promise that on its own: rows inserted after the deletes still live, and depending on ordering discipline in every handler that creates authentication state is fragile. Instead, `users.auth_epoch` is a monotonic credential version. Sessions and login challenges carry the epoch they were issued under, `store::active_session` accepts a session only while its epoch matches the current epoch of the user, and redemption bumps the epoch in the same batch that consumes the token and rewrites the password. Invalidation becomes a property of validation at the single session-lookup choke point, so a handler that misses a check can still only produce a row that never authenticates. An authenticated password change deliberately does not bump the epoch, because it keeps the current session; it gates its update on the credential it verified so it cannot overwrite a reset that lands mid-request.

## Considered Options

- **Conditional statements only, no migration**: make every session and challenge insert conditional on `password_phc` still matching the verified hash, following the verify-email idiom. Rejected because the guarantee would depend on four separately ordered handlers, and challenge completion — today a consume step followed by a separate session insert — could not be made safe without restructuring.

## Consequences

- A new migration adds the epoch column to `users`, `sessions`, and `login_challenges`, and the session and challenge helpers carry it.
- The epoch is the natural primitive for a future sign-out-everywhere operation.
