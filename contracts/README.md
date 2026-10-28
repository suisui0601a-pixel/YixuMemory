# Contract scope

Resource JSON schemas are server response/persistence DTO shapes, not permission grants or raw write bodies. `$id` is a logical identifier, not a live hosting promise; checks load local files and require no resolver network.
OpenAPI embeds identical resource shapes for portable validation; drift is tested. Resource schemas describe available references; erased resources are served as 404/410 or minimal Deletion status, never as an unvalidated partial Revision.
Relational, authorization, time ordering, classification lineage and deletion-closure rules require application/database integration tests. Source locators are UUID references to erasable locator Payloads.
Write commands reference already-authorized staged payloads/evidence; ingestion creates Event+Evidence atomically. There is no endpoint accepting arbitrary source-free Memory text.

P0-SC: payload.schema.json is internal production protection metadata; deletion-request.schema.json is the accepted server scope/coverage result. Explicit request intent and design matrix are in security/. Deletion remains bounded to a Space object graph plus all controlled copies; unsupported source/subject-wide intents fail before execution. Classification declarations never grant plaintext storage. Shape flags/receipt refs do not prove encryption or erasure. See ADR-010/011 and P0_SECURITY_ACCEPTANCE.
