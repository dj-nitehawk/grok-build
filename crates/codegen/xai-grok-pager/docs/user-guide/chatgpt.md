# ChatGPT Plus/Pro (Codex backend)

Unofficial. This is the same ChatGPT subscription path OpenCode uses: Codex CLI's OAuth client and `https://chatgpt.com/backend-api/codex/responses`. OpenAI can break it without notice. It is **not** `OPENAI_API_KEY` and not official OpenAI support.

Grok models still use `grok login`. Image, video, voice, embeddings, and billing UI stay on xAI.

## Sign in

```bash
grok chatgpt-login
```

That opens a browser against `auth.openai.com` and atomically writes tokens to `~/.grok/chatgpt-auth.json` (mode 0600). You should see `signed in as …`. Token refreshes are coordinated across Grok processes. If credentials have expired and cannot be refreshed, sign in again.

Headless or when port **1455** is already bound (OpenCode and Codex CLI use the same callback port):

```bash
grok chatgpt-login --device
```

Sign out:

```bash
grok chatgpt-logout
```

If logout reports that credentials are busy, a refresh holds the credential lock. Retry logout after that refresh finishes; a failed logout has not removed the credentials.

`/chatgpt-login` in the TUI prints these commands. Login cannot run inside the TUI because it binds `localhost:1455`.

## Use GPT-6 Astra

```bash
grok models          # lists gpt-6-astra plus the usual Grok defaults
grok -m gpt-6-astra -p "reply with pong"
```

In a session: `/model gpt-6-astra` or Ctrl+M. After picking Astra, `/model` chains into a reasoning-effort menu (`low`, `medium`, `high`, `xhigh`). Default is `medium`. Astra rejects `none` (HTTP 400). You can also set it later with `/effort high`. Switching Grok ↔ GPT compact-on-family-change (`xai` → `openai`).

```
/model GPT-6 Astra
/model GPT-6 Astra high
/effort xhigh
```

Only `gpt-6-astra` is seeded. There are no gpt-5.x aliases. If the Codex backend rejects the slug, that is a live eligibility failure, not a missing fallback.

## Override

User `[model.gpt-6-astra]` still wins via the normal merge. Example:

```toml
[model.gpt-6-astra]
context_window = 272000
```

Do not point this model at `api.openai.com` unless you intend a different, API-key path. A custom endpoint using `auth_provider = "chatgpt"` is an explicit choice to send ChatGPT credentials to that endpoint, and must use HTTPS. Only the standard ChatGPT Codex URL automatically receives Codex-specific compatibility settings.

## What this does not do

- It does not replace `grok login`.
- It does not send your Grok session token to chatgpt.com.
- It does not enable xAI-only Responses extras (`reasoning.encrypted_content`, hosted search) on the Codex URL.
