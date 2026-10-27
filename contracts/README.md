# Contract scope

Resource JSON schemas are server response/persistence DTO shapes, not permission grants or raw write bodies. `$id` is a logical identifier, not a live hosting promise; checks load local files and require no resolver network.
OpenAPI embeds identical resource shapes for portable validation; drift is tested. Resource schemas describe available references; erased resources are served as 404/410 or minimal Deletion status, never as an unvalidated partial Revision.
Relational, authorization, time ordering, classification lineage and deletion-closure rules require application/database integration tests. Source locators are UUID references to erasable locator Payloads.
Write commands reference already-authorized staged payloads/evidence; ingestion creates Event+Evidence atomically. There is no endpoint accepting arbitrary source-free Memory text.
