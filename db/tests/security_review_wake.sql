-- One automatic wake of the owner's chat (security_review.sql), then the
-- 30-second rate limit is reset so the next step can wake it again.
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('9a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT set_config('test.woken', (current_setting('test.woken')::int
       + jsonb_array_length(app.claim_chat_messages(:'ws', :'chat_owner', true)->'messages'))::text, false) \gset
COMMIT;
RESET ROLE;
UPDATE chats SET last_wake_at = NULL WHERE id = :'chat_owner';
SET ROLE aidrive_app;
