---
id: 9f78caad-67ed-48fb-b356-d3684aab214b
title: "MCP tools must require explicit repo parameter, no fallbacks"
type: rule
status: active
created_at: "2026-09-05T07:08:13.895469759+00:00"
updated_at: "2026-09-05T07:08:13.895469759+00:00"
references: []
confidence: 0.9
verified_against: ["6418285a-aa22-5b7a-ba4b-c68c49fd6aaa=4a09a3779b76dde9a5813dc8408095de6c7b8b548b2d9ee645f306fa0b3a5b20", "e9837ff8-487d-5a4b-b76b-d7b8cf9ff290=dea18cd35f6f8219b080e56096a315c75d294e4340b169f35d9536d308dc165e", "feca2a7c-1249-5d0f-b68f-295e59d141bd=87daebe7a505757b279cf42767603aca0a89297ce90bc38625a5b14e99606747"]
---

Every MCP tool must require an explicit `repo` parameter. No fallback to cwd, no implicit session context, no Roots-based discovery. This aligns with the 2026-07-28 MCP spec (SEP-2577) which deprecated Roots in favor of tool parameters, resource URIs, or server configuration. The explicit parameter approach is simpler, less ambiguous, and future-proof — an agent working across multiple repos must state which repo it means.