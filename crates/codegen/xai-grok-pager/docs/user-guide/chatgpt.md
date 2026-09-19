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

## Use GPT-6 Astra or GPT-6 Sol

```bash
grok models          # lists gpt-6-astra and gpt-6-sol plus the usual Grok defaults
grok -m gpt-6-sol -p "reply with pong"
```

In a session: `/model gpt-6-sol` (or `gpt-6-astra`) or Ctrl+M. After picking a Codex model, `/model` chains into a reasoning-effort menu. Default is `medium`. Codex rejects `none` (HTTP 400). Sol also offers `max`. You can set effort later with `/effort high`. Switching Grok ↔ GPT compact-on-family-change (`xai` → `openai`).

```
/model GPT-6 Sol
/model GPT-6 Sol high
/effort max
/model GPT-6 Astra xhigh
```

Seeded slugs are `gpt-6-astra` and `gpt-6-sol`. `gpt-5.6-sol` is not seeded. If the Codex backend rejects the slug, that is a live eligibility failure, not a missing fallback.

## Override

User `[model.gpt-6-astra]` or `[model.gpt-6-sol]` still wins via the normal merge. Example:

```toml
[model.gpt-6-sol]
context_window = 1050000
```

Do not point this model at `api.openai.com` unless you intend a different, API-key path. A custom endpoint using `auth_provider = "chatgpt"` is an explicit choice to send ChatGPT credentials to that endpoint, and must use HTTPS. Only the standard ChatGPT Codex URL automatically receives Codex-specific compatibility settings.

## What this does not do

- It does not replace `grok login`.
- It does not send your Grok session token to chatgpt.com.
- It does not enable xAI-only Responses extras (`reasoning.encrypted_content`, hosted search) on the Codex URL.
