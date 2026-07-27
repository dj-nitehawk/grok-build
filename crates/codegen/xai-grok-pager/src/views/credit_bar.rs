//! Credit balance indicator for the agent status bar.
//!
//! Shows the user's coding credit usage as a compact status bar item.
//! Fetches real data from the `x.ai/billing` agent extension.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// Credit balance state from the billing API.
#[derive(Debug, Clone)]
pub struct CreditBalance {
    /// Usage as a percentage of the allowance (0.0 to 100.0).
    pub usage_pct: f64,
    /// Usage as a percentage of total budget (free and on-demand when enabled).
    pub effective_usage_pct: f64,
    /// Billing period end as a formatted local wall-clock string (no zone label), e.g. "Mar 31, 12:00".
    pub period_end_display: Option<String>,
    /// Absolute period end (UTC) for compact reset-remaining chips
    /// (`format_quota_chip`). Prefer this over re-parsing [`Self::period_end_display`].
    pub period_end: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether pay-as-you-go (on-demand) billing is enabled.
    pub pay_as_you_go: bool,
    /// On-demand spending cap in USD cents (e.g. 500 is $5.00).
    pub on_demand_cap_cents: Option<i64>,
    /// On-demand usage this period in USD cents.
    pub on_demand_used_cents: Option<i64>,
    /// Remaining prepaid ("bought") credit balance in USD cents.
    pub prepaid_balance_cents: Option<i64>,
    /// Usage period type from the billing response (the proto enum name, e.g. `USAGE_PERIOD_TYPE_WEEKLY`).
    /// Drives the "Weekly/Monthly limit" label.
    pub period_type: Option<String>,
    /// From credits config `is_unified_billing_user` (`None` if absent).
    /// `Some(true)` means the unified pool and buy-credits UX; `Some(false)` means the legacy on-demand (PAYG) UX.
    pub is_unified_billing_user: Option<bool>,
}

impl CreditBalance {
    /// Label for the percentage allowance, chosen from the period type: "Weekly limit" / "Monthly limit", falling back to "Usage" when unknown.
    pub fn usage_label(&self) -> &'static str {
        match self.period_type.as_deref() {
            Some(t) if t.contains("WEEKLY") => "Weekly limit",
            Some(t) if t.contains("MONTHLY") => "Monthly limit",
            _ => "Usage",
        }
    }
}

/// Compact remaining-time for the info-line chip: `4d5h`, `5h12m`, `45m`, `<1m`.
///
/// Uses whole units only (no seconds) so the chip stays short on the border.
pub fn format_reset_remaining(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        if hours > 0 {
            format!("{days}d{hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        if mins > 0 {
            format!("{hours}h{mins}m")
        } else {
            format!("{hours}h")
        }
    } else if mins > 0 {
        format!("{mins}m")
    } else {
        "<1m".to_string()
    }
}

/// Prompt info-line quota chip: `10% (reset: 4d5h)`, or `10%` without a period end.
///
/// Floors the percentage to match the `/usage` summary and SpendingLimiter
/// truncation (`99.994%` → `99%`).
pub fn format_quota_chip(balance: &CreditBalance) -> String {
    format_quota_chip_at(balance, chrono::Utc::now())
}

/// Like [`format_quota_chip`], with an explicit `now` for tests.
pub fn format_quota_chip_at(
    balance: &CreditBalance,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let pct = balance.usage_pct.floor() as i64;
    match balance.period_end {
        Some(end) => {
            let secs = (end - now).num_seconds().max(0) as u64;
            format!("{pct}% (reset: {})", format_reset_remaining(secs))
        }
        None => format!("{pct}%"),
    }
}

/// Prompt info-line quota chip text.
///
/// Hidden when the billing surface is off or this is a gateway chat session.
/// While a fetch is in flight (Alt+Q), shows `"refreshing..."` so the user
/// sees progress without scrollback spam. Otherwise the cached balance chip,
/// or nothing if uncached.
pub fn quota_chip_for_prompt(
    billing_surface_visible: bool,
    chat_kind: bool,
    fetch_in_flight: bool,
    balance: Option<&CreditBalance>,
) -> Option<String> {
    if !billing_surface_visible || chat_kind {
        return None;
    }
    if fetch_in_flight {
        return Some("refreshing...".to_string());
    }
    balance.map(format_quota_chip)
}

/// Context-window chip for the prompt info line: `"47K / 500K (9%)"` plus
/// the usage-urgency gradient color.
///
/// Omitted for gateway chat sessions (remote owns context) and when used/total
/// are not both known with a positive total. Lives here so agent render is a
/// thin call site rather than inlining context-bar math into hot upstream files.
pub fn context_chip_for_prompt(
    chat_kind: bool,
    used: Option<u64>,
    total: Option<u64>,
    theme: &Theme,
) -> Option<(String, ratatui::style::Color)> {
    if chat_kind {
        return None;
    }
    let (used, total) = match (used, total) {
        (Some(used), Some(total)) if total > 0 => (used, total),
        _ => return None,
    };
    use crate::views::context_bar;
    let pct = xai_token_estimation::usage_percentage(used, total);
    let text = format!(
        "{} / {} ({:.0}%)",
        context_bar::fmt_tokens(used),
        context_bar::fmt_tokens(total),
        pct
    );
    let breakpoints = context_bar::default_breakpoints(theme);
    let color = crate::theme::quantize(context_bar::blend_color(pct, &breakpoints));
    Some((text, color))
}

/// Whether a session mode flag should appear on the prompt info line.
///
/// Always-approve is omitted: many sessions leave it on permanently (e.g.
/// sandbox), so the chip is constant noise. Plan and auto still surface.
/// Shared by full-TUI [`PromptBorderChips`] and minimal mode.
pub fn keep_info_line_mode_flag(text: &str) -> bool {
    text != "always-approve"
}

/// Owned context + quota chips for the prompt bottom-border info line.
///
/// Keeps fork-only assembly out of `agent_view/render.rs`: that call site
/// only builds mode flags and weaves them through here.
pub struct PromptBorderChips {
    /// Context-window chip text + urgency color, when known.
    pub context: Option<(String, ratatui::style::Color)>,
    /// Billing quota chip text (or `"refreshing..."`), when shown.
    pub quota: Option<String>,
}

impl PromptBorderChips {
    /// Build chips from agent session state used by the prompt info line.
    pub fn for_prompt(
        chat_kind: bool,
        context_used: Option<u64>,
        context_total: Option<u64>,
        theme: &Theme,
        billing_surface_visible: bool,
        billing_fetch_in_flight: bool,
        balance: Option<&CreditBalance>,
    ) -> Self {
        Self {
            context: context_chip_for_prompt(chat_kind, context_used, context_total, theme),
            quota: quota_chip_for_prompt(
                billing_surface_visible,
                chat_kind,
                billing_fetch_in_flight,
                balance,
            ),
        }
    }

    /// Order: context · mode flags · quota (normal / editing-queued modes).
    ///
    /// Always-approve is dropped via [`keep_info_line_mode_flag`] so
    /// `agent_view/render` can keep matching main (it still builds that flag).
    pub fn with_mode_flags<'a>(
        &'a self,
        mut mode_flags: Vec<crate::views::prompt_widget::PromptFlag<'a>>,
    ) -> Vec<crate::views::prompt_widget::PromptFlag<'a>> {
        use crate::views::prompt_widget::PromptFlag;
        mode_flags.retain(|f| keep_info_line_mode_flag(f.text));
        let mut out = Vec::with_capacity(mode_flags.len() + 2);
        if let Some((ref text, color)) = self.context {
            out.push(PromptFlag {
                text: text.as_str(),
                color: Some(color),
                bold: false,
            });
        }
        out.append(&mut mode_flags);
        if let Some(ref text) = self.quota {
            out.push(PromptFlag {
                text: text.as_str(),
                color: None,
                bold: false,
            });
        }
        out
    }

    /// Context + quota only (bash / shell prompt override: drop mode flags).
    pub fn chip_only_flags<'a>(&'a self) -> Vec<crate::views::prompt_widget::PromptFlag<'a>> {
        use crate::views::prompt_widget::PromptFlag;
        let mut out = Vec::with_capacity(2);
        if let Some((ref text, color)) = self.context {
            out.push(PromptFlag {
                text: text.as_str(),
                color: Some(color),
                bold: false,
            });
        }
        if let Some(ref text) = self.quota {
            out.push(PromptFlag {
                text: text.as_str(),
                color: None,
                bold: false,
            });
        }
        out
    }
}

/// Auto top-up rule data used by the `/usage` summary.
#[derive(Debug, Clone)]
pub struct AutoTopupInfo {
    /// Whether auto top-up is enabled.
    pub enabled: bool,
    /// Per-trigger top-up amount in USD cents.
    pub topup_amount_cents: Option<i64>,
    /// Optional maximum monthly top-up amount in USD cents.
    pub max_amount_cents: Option<i64>,
}

impl AutoTopupInfo {
    /// A known "auto top-up disabled" state, distinct from an unresolved `None`, which means the rule hasn't been fetched yet.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            topup_amount_cents: None,
            max_amount_cents: None,
        }
    }
}

/// Outcome of an auto top-up rule fetch, so a transient failure doesn't clear a previously cached rule.
#[derive(Debug, Clone)]
pub enum AutoTopupFetch {
    /// A definitive rule state (a real rule, or [`AutoTopupInfo::disabled`] when the backend reports none).
    /// Stored as the *known* auto top-up state.
    Resolved(AutoTopupInfo),
    /// Fetch failed; keep the cached value.
    /// A stored `None` therefore means "not yet known", not "no auto top-up".
    Unchanged,
    /// The rule is not applicable (no prepaid credits); reset the cache to "unknown" so a later credits period doesn't read a stale rule.
    Cleared,
}

/// Format `cents` as a dollar string: whole dollars as `$N`, otherwise `$N.NN`.
fn fmt_dollars(cents: i64) -> String {
    let dollars = cents as f64 / 100.0;
    if dollars.fract() == 0.0 {
        format!("${dollars:.0}")
    } else {
        format!("${dollars:.2}")
    }
}

/// Build the `/usage` summary block shown in scrollback.
///
/// Always shows usage % and (when known) the next reset time.
/// The credits block is rendered only when the user has a positive prepaid balance:
/// - no prepaid balance       → credits block omitted entirely
/// - auto top-up off/unknown  → `Auto topup: disabled` (no max line)
/// - auto top-up on, no max   → `Auto topup: $N`
/// - auto top-up on, max set  → `Auto topup: $N` + `Max monthly topup: $M`
pub fn format_usage_summary(balance: &CreditBalance, autotopup: Option<&AutoTopupInfo>) -> String {
    // Floor to match the backend SpendingLimiter's `as u8` truncation (99.994% renders as 99%, never 100% until truly exhausted)
    let mut lines = vec![format!(
        "{}: {}%",
        balance.usage_label(),
        balance.usage_pct.floor() as i64
    )];
    if let Some(reset) = &balance.period_end_display {
        lines.push(format!("Next reset: {reset}"));
    }

    // Billing stores credit / top-up amounts as negative cents (accounting convention); display the absolute USD value, matching the web clients
    if let Some(prepaid) = balance
        .prepaid_balance_cents
        .map(i64::abs)
        .filter(|c| *c > 0)
    {
        lines.push(String::new());
        lines.push(format!("Credits: {}", fmt_dollars(prepaid)));
        match autotopup {
            Some(at) if at.enabled && at.topup_amount_cents.is_some() => {
                lines.push(format!(
                    "Auto topup: {}",
                    fmt_dollars(at.topup_amount_cents.unwrap().abs())
                ));
                if let Some(max) = at.max_amount_cents {
                    lines.push(format!("Max monthly topup: {}", fmt_dollars(max.abs())));
                }
            }
            _ => lines.push("Auto topup: disabled".to_string()),
        }
    }

    // Legacy on-demand (pay-as-you-go) billing, shown only when enabled, for users on the older monthly and on-demand model
    // Amounts always carry cents (e.g. `$50.00`), matching the web client.
    if balance.pay_as_you_go {
        let used = balance.on_demand_used_cents.unwrap_or(0).abs() as f64 / 100.0;
        let cap = balance.on_demand_cap_cents.unwrap_or(0).abs() as f64 / 100.0;
        lines.push(String::new());
        lines.push(format!("Pay-as-you-go: ${used:.2} used of ${cap:.2} limit"));
    }

    lines.join("\n")
}

/// Low-balance ($10) and pay-as-you-go critical ($5) warning thresholds, in cents.
const LOW_BALANCE_CENTS: i64 = 1000;
const PAY_AS_YOU_GO_CRITICAL_CENTS: i64 = 500;

/// The prompt's usage/credits warning as `(text, critical)`, or `None`.
/// `critical` renders yellow, else grey; team users with `usage_visible = false` never warn.
/// Behaviour splits by billing model: prepaid credits, pay-as-you-go on-demand, or the included-allowance percentage.
/// The unit tests pin the exact thresholds and copy.
///
/// Gateway light-frontend (`kind: "chat"`) sessions must not show Build coding-credit warnings.
/// Use [`usage_warning_for_session`] with `gateway_chat = true` so the prompt shows no fake local sampler telemetry.
pub fn usage_warning(
    balance: &CreditBalance,
    autotopup: Option<&AutoTopupInfo>,
    usage_visible: bool,
) -> Option<(String, bool)> {
    usage_warning_for_session(balance, autotopup, usage_visible, false)
}

/// Like [`usage_warning`], but suppresses output for gateway/chat-kind sessions.
pub fn usage_warning_for_session(
    balance: &CreditBalance,
    autotopup: Option<&AutoTopupInfo>,
    usage_visible: bool,
    gateway_chat: bool,
) -> Option<(String, bool)> {
    if gateway_chat || !usage_visible {
        return None;
    }

    // A non-zero prepaid balance (stored as signed cents) means the credits model.
    let credits = balance
        .prepaid_balance_cents
        .map(i64::abs)
        .filter(|c| *c > 0);

    let Some(credits_cents) = credits else {
        // Pay-as-you-go (legacy on-demand): warn on dollars left in the cap once the included allowance is spent
        if balance.pay_as_you_go {
            if balance.usage_pct >= 100.0 {
                let cap = balance.on_demand_cap_cents.unwrap_or(0).abs();
                let used = balance.on_demand_used_cents.unwrap_or(0).abs();
                let remaining = (cap - used).max(0);
                if remaining <= LOW_BALANCE_CENTS {
                    let text = format!("Pay-as-you-go limit left: {}", fmt_dollars(remaining));
                    return Some((text, remaining <= PAY_AS_YOU_GO_CRITICAL_CENTS));
                }
            }
            return None;
        }

        let pct = balance.effective_usage_pct;
        if pct > 90.0 {
            // "Left" is the complement of floored usage, so it agrees with the floored summary: 99.994% shows "1% left", not "0%"
            let remaining = (100 - pct.floor() as i64).max(0);
            let label = balance.usage_label();
            return Some((format!("{label} left: {remaining}%"), pct > 95.0));
        }
        return None;
    };

    // Credits are only drawn down at 100% usage; don't warn before then.
    if balance.usage_pct < 100.0 {
        return None;
    }

    let credits_warning = || {
        (
            format!("Credits left: {}", fmt_dollars(credits_cents)),
            true,
        )
    };

    // Auto top-up gates the warning: unknown stays silent; disabled warns when low
    // Enabled without a max never warns; enabled with a max warns below one top-up amount
    match autotopup {
        None => None,
        Some(at) if !at.enabled => (credits_cents <= LOW_BALANCE_CENTS).then(credits_warning),
        Some(at) if at.max_amount_cents.is_none() => None,
        Some(at) => at
            .topup_amount_cents
            .map(i64::abs)
            .and_then(|amt| (credits_cents < amt).then(credits_warning)),
    }
}

/// Build the credit balance indicator as a `Line<'static>`.
///
/// Shows `Credits used: XX%` in the status bar.
///
/// Gateway light-frontend (`kind: "chat"`) sessions must not show Build coding credits.
/// Use [`credit_bar_line_for_session`] with `gateway_chat = true`, which returns `None`.
/// Remote settings or a managed opt-in for chat entry can share the same gate later; for now it only suppresses misleading local telemetry.
pub fn credit_bar_line(balance: &CreditBalance, hovered: bool, theme: &Theme) -> Line<'static> {
    credit_bar_line_for_session(balance, hovered, theme, false)
        .expect("non-chat credit_bar_line always renders")
}

/// Like [`credit_bar_line`], but returns `None` for gateway/chat-kind sessions so the status bar never implies Build sampler / coding-credit usage.
pub fn credit_bar_line_for_session(
    balance: &CreditBalance,
    _hovered: bool,
    theme: &Theme,
    gateway_chat: bool,
) -> Option<Line<'static>> {
    if gateway_chat {
        return None;
    }
    let pct = balance.usage_pct;
    let color = if pct >= 100.0 {
        theme.accent_error
    } else if pct >= 80.0 {
        theme.warning
    } else {
        theme.accent_success
    };

    let text = format!("Credits used: {pct:.0}%");

    let style = Style::default().fg(color).bg(theme.bg_base);
    Some(Line::from(Span::styled(text, style)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bal(pct: f64) -> CreditBalance {
        CreditBalance {
            usage_pct: pct,
            effective_usage_pct: pct,
            period_end_display: None,
            period_end: None,
            pay_as_you_go: false,
            on_demand_cap_cents: None,
            on_demand_used_cents: None,
            prepaid_balance_cents: None,
            period_type: None,
            is_unified_billing_user: None,
        }
    }

    fn topup(enabled: bool, amount: Option<i64>, max: Option<i64>) -> AutoTopupInfo {
        AutoTopupInfo {
            enabled,
            topup_amount_cents: amount,
            max_amount_cents: max,
        }
    }

    #[test]
    fn summary_no_credits_omits_credits_block() {
        let b = CreditBalance {
            period_end_display: Some("June 14, 16:00".into()),
            prepaid_balance_cents: Some(0),
            ..bal(25.0)
        };
        // Even with an auto-topup rule present, zero prepaid omits the credits block
        let out = format_usage_summary(&b, Some(&topup(true, Some(2000), Some(10000))));
        assert_eq!(out, "Usage: 25%\nNext reset: June 14, 16:00");
    }

    #[test]
    fn summary_credits_without_autotopup_shows_disabled() {
        let b = CreditBalance {
            prepaid_balance_cents: Some(10000),
            ..bal(25.0)
        };
        assert_eq!(
            format_usage_summary(&b, None),
            "Usage: 25%\n\nCredits: $100\nAuto topup: disabled"
        );
        // A disabled rule renders the same.
        assert_eq!(
            format_usage_summary(&b, Some(&topup(false, Some(2000), Some(10000)))),
            "Usage: 25%\n\nCredits: $100\nAuto topup: disabled"
        );
    }

    #[test]
    fn summary_autotopup_enabled_without_max_omits_max() {
        let b = CreditBalance {
            prepaid_balance_cents: Some(10000),
            ..bal(25.0)
        };
        assert_eq!(
            format_usage_summary(&b, Some(&topup(true, Some(2000), None))),
            "Usage: 25%\n\nCredits: $100\nAuto topup: $20"
        );
    }

    #[test]
    fn summary_autotopup_enabled_with_max_renders_all() {
        let b = CreditBalance {
            period_end_display: Some("June 14, 16:00".into()),
            prepaid_balance_cents: Some(10000),
            ..bal(25.0)
        };
        assert_eq!(
            format_usage_summary(&b, Some(&topup(true, Some(2000), Some(10000)))),
            "Usage: 25%\nNext reset: June 14, 16:00\n\nCredits: $100\nAuto topup: $20\nMax monthly topup: $100"
        );
    }

    #[test]
    fn summary_formats_fractional_dollars() {
        let b = CreditBalance {
            prepaid_balance_cents: Some(1250),
            ..bal(25.0)
        };
        assert_eq!(
            format_usage_summary(&b, Some(&topup(true, Some(550), None))),
            "Usage: 25%\n\nCredits: $12.50\nAuto topup: $5.50"
        );
    }

    #[test]
    fn summary_abs_negative_billing_amounts() {
        // Billing returns credit / top-up amounts as negative cents; the summary must render them as positive USD (matching the web)
        let b = CreditBalance {
            prepaid_balance_cents: Some(-500),
            ..bal(100.0)
        };
        assert_eq!(
            format_usage_summary(&b, Some(&topup(true, Some(-500), Some(-1000)))),
            "Usage: 100%\n\nCredits: $5\nAuto topup: $5\nMax monthly topup: $10"
        );
    }

    #[test]
    fn summary_pay_as_you_go_enabled_renders_used_of_limit() {
        let b = CreditBalance {
            pay_as_you_go: true,
            on_demand_used_cents: Some(355),
            on_demand_cap_cents: Some(5000),
            period_type: Some("USAGE_PERIOD_TYPE_MONTHLY".into()),
            period_end_display: Some("June 30, 16:00".into()),
            ..bal(91.0)
        };
        assert_eq!(
            format_usage_summary(&b, None),
            "Monthly limit: 91%\nNext reset: June 30, 16:00\n\nPay-as-you-go: $3.55 used of $50.00 limit"
        );
    }

    #[test]
    fn summary_pay_as_you_go_disabled_omits_line() {
        let b = CreditBalance {
            pay_as_you_go: false,
            period_type: Some("USAGE_PERIOD_TYPE_MONTHLY".into()),
            period_end_display: Some("June 30, 16:00".into()),
            ..bal(91.0)
        };
        assert_eq!(
            format_usage_summary(&b, None),
            "Monthly limit: 91%\nNext reset: June 30, 16:00"
        );
    }

    // ── usage_label / period type ────────────────────────────────────

    fn bal_period(pct: f64, period_type: &str) -> CreditBalance {
        CreditBalance {
            period_type: Some(period_type.to_string()),
            ..bal(pct)
        }
    }

    #[test]
    fn usage_label_from_period_type() {
        assert_eq!(
            bal_period(0.0, "USAGE_PERIOD_TYPE_WEEKLY").usage_label(),
            "Weekly limit"
        );
        assert_eq!(
            bal_period(0.0, "USAGE_PERIOD_TYPE_MONTHLY").usage_label(),
            "Monthly limit"
        );
        // Unknown / unspecified / absent falls back to "Usage"
        assert_eq!(
            bal_period(0.0, "USAGE_PERIOD_TYPE_UNSPECIFIED").usage_label(),
            "Usage"
        );
        assert_eq!(bal(0.0).usage_label(), "Usage");
    }

    #[test]
    fn summary_uses_period_label() {
        let weekly = bal_period(25.0, "USAGE_PERIOD_TYPE_WEEKLY");
        assert_eq!(format_usage_summary(&weekly, None), "Weekly limit: 25%");
        let monthly = bal_period(25.0, "USAGE_PERIOD_TYPE_MONTHLY");
        assert_eq!(format_usage_summary(&monthly, None), "Monthly limit: 25%");
    }

    #[test]
    fn warning_uses_period_label() {
        let weekly = bal_period(92.0, "USAGE_PERIOD_TYPE_WEEKLY");
        assert_eq!(
            usage_warning(&weekly, None, true),
            Some(("Weekly limit left: 8%".to_string(), false))
        );
    }

    #[test]
    fn summary_floors_usage_percent() {
        // Match the backend SpendingLimiter (`as u8` truncation): 99.994% must render as 99%, not round up to 100%
        let almost = bal_period(99.994, "USAGE_PERIOD_TYPE_WEEKLY");
        assert_eq!(format_usage_summary(&almost, None), "Weekly limit: 99%");
        // A true 100% still shows 100%.
        let full = bal_period(100.0, "USAGE_PERIOD_TYPE_WEEKLY");
        assert_eq!(format_usage_summary(&full, None), "Weekly limit: 100%");
    }

    #[test]
    fn warning_percent_left_is_floor_complement() {
        // 99.994% used floors to 99%, so it shows "1% left" (not "0% left"), and the warning and the floored summary always sum to 100
        let almost = bal_period(99.994, "USAGE_PERIOD_TYPE_WEEKLY");
        assert_eq!(
            usage_warning(&almost, None, true),
            Some(("Weekly limit left: 1%".to_string(), true))
        );
        // A true 100% (no credits) shows "0% left"
        let full = bal_period(100.0, "USAGE_PERIOD_TYPE_WEEKLY");
        assert_eq!(
            usage_warning(&full, None, true),
            Some(("Weekly limit left: 0%".to_string(), true))
        );
    }

    // ── usage_warning (prompt info row) ──────────────────────────────

    #[test]
    fn warning_usage_model_thresholds() {
        assert_eq!(usage_warning(&bal(50.0), None, true), None);
        assert_eq!(
            usage_warning(&bal(92.0), None, true),
            Some(("Usage left: 8%".to_string(), false))
        );
        assert_eq!(
            usage_warning(&bal(97.0), None, true),
            Some(("Usage left: 3%".to_string(), true))
        );
    }

    #[test]
    fn warning_hidden_for_team_users() {
        assert_eq!(usage_warning(&bal(99.0), None, false), None);
        let credits = CreditBalance {
            prepaid_balance_cents: Some(100),
            ..bal(0.0)
        };
        assert_eq!(usage_warning(&credits, None, false), None);
    }

    #[test]
    fn warning_credits_unknown_topup_is_suppressed() {
        // At 100% usage with prepaid credits but the rule not yet known (None), never warn; it resolves on the next billing fetch
        let b = CreditBalance {
            prepaid_balance_cents: Some(100),
            ..bal(100.0)
        };
        assert_eq!(usage_warning(&b, None, true), None);
    }

    #[test]
    fn warning_credits_suppressed_below_full_usage() {
        // Low credits and no auto top-up, but the included allowance still has room (usage < 100%), so no warning; credits aren't being spent yet
        let disabled = topup(false, None, None);
        let low = CreditBalance {
            prepaid_balance_cents: Some(453),
            ..bal(0.0)
        };
        assert_eq!(usage_warning(&low, Some(&disabled), true), None);
        // The same balance warns once the allowance is exhausted
        let exhausted = CreditBalance {
            prepaid_balance_cents: Some(453),
            ..bal(100.0)
        };
        assert_eq!(
            usage_warning(&exhausted, Some(&disabled), true),
            Some(("Credits left: $4.53".to_string(), true))
        );
    }

    #[test]
    fn warning_credits_no_topup_low_shows_dollars() {
        // "No auto top-up" is a known, disabled rule (not an unresolved None).
        let b = CreditBalance {
            prepaid_balance_cents: Some(453),
            ..bal(100.0)
        };
        let disabled = topup(false, None, None);
        assert_eq!(
            usage_warning(&b, Some(&disabled), true),
            Some(("Credits left: $4.53".to_string(), true))
        );
    }

    #[test]
    fn warning_credits_no_topup_above_threshold_silent() {
        let disabled = topup(false, None, None);
        let b = CreditBalance {
            prepaid_balance_cents: Some(1500),
            ..bal(100.0)
        };
        assert_eq!(usage_warning(&b, Some(&disabled), true), None);
        // Exactly $10 is still "low".
        let at_ten = CreditBalance {
            prepaid_balance_cents: Some(1000),
            ..bal(100.0)
        };
        assert_eq!(
            usage_warning(&at_ten, Some(&disabled), true),
            Some(("Credits left: $10".to_string(), true))
        );
    }

    #[test]
    fn warning_credits_topup_no_max_never_warns() {
        let b = CreditBalance {
            prepaid_balance_cents: Some(1),
            ..bal(100.0)
        };
        assert_eq!(
            usage_warning(&b, Some(&topup(true, Some(2000), None)), true),
            None
        );
    }

    #[test]
    fn warning_credits_topup_with_max_below_topup_amount() {
        // $15 balance, $20 top-up amount, $100 max: below one top-up, so it warns
        let b = CreditBalance {
            prepaid_balance_cents: Some(1500),
            ..bal(100.0)
        };
        assert_eq!(
            usage_warning(&b, Some(&topup(true, Some(2000), Some(10000))), true),
            Some(("Credits left: $15".to_string(), true))
        );
        let plenty = CreditBalance {
            prepaid_balance_cents: Some(2500),
            ..bal(100.0)
        };
        assert_eq!(
            usage_warning(&plenty, Some(&topup(true, Some(2000), Some(10000))), true),
            None
        );
    }

    #[test]
    fn warning_credits_handles_negative_cents() {
        let b = CreditBalance {
            prepaid_balance_cents: Some(-453),
            ..bal(100.0)
        };
        assert_eq!(
            usage_warning(&b, Some(&topup(true, Some(-2000), Some(-10000))), true),
            Some(("Credits left: $4.53".to_string(), true))
        );
    }

    #[test]
    fn warning_credits_take_precedence_over_usage() {
        // A credits user below 100% usage gets no warning: no usage-% warning, and credits aren't being spent yet
        // A non-credits user would see "Usage left: 1%" at 99%
        let b = CreditBalance {
            prepaid_balance_cents: Some(5000),
            ..bal(99.0)
        };
        assert_eq!(
            usage_warning(&b, Some(&topup(false, None, None)), true),
            None
        );
        // Zero prepaid falls back to the usage model.
        let zero = CreditBalance {
            prepaid_balance_cents: Some(0),
            ..bal(99.0)
        };
        assert_eq!(
            usage_warning(&zero, None, true),
            Some(("Usage left: 1%".to_string(), true))
        );
    }

    // ── usage_warning: pay-as-you-go (monthly on-demand) ─────────────

    fn pay_as_you_go(usage_pct: f64, cap_cents: i64, used_cents: i64) -> CreditBalance {
        CreditBalance {
            pay_as_you_go: true,
            on_demand_cap_cents: Some(cap_cents),
            on_demand_used_cents: Some(used_cents),
            period_type: Some("USAGE_PERIOD_TYPE_MONTHLY".into()),
            ..bal(usage_pct)
        }
    }

    #[test]
    fn warning_pay_as_you_go_low_dollars_shows_remaining() {
        // $50 cap, $42 used leaves $8, shown grey (above $5)
        let grey = pay_as_you_go(100.0, 5000, 4200);
        assert_eq!(
            usage_warning(&grey, None, true),
            Some(("Pay-as-you-go limit left: $8".to_string(), false))
        );
        // $50 cap, $46 used leaves $4, critical (yellow)
        let yellow = pay_as_you_go(100.0, 5000, 4600);
        assert_eq!(
            usage_warning(&yellow, None, true),
            Some(("Pay-as-you-go limit left: $4".to_string(), true))
        );
    }

    #[test]
    fn warning_pay_as_you_go_boundaries() {
        // Exactly $10 left shows the warning, grey
        let at_ten = pay_as_you_go(100.0, 5000, 4000);
        assert_eq!(
            usage_warning(&at_ten, None, true),
            Some(("Pay-as-you-go limit left: $10".to_string(), false))
        );
        // Exactly $5 left is critical (yellow)
        let at_five = pay_as_you_go(100.0, 5000, 4500);
        assert_eq!(
            usage_warning(&at_five, None, true),
            Some(("Pay-as-you-go limit left: $5".to_string(), true))
        );
    }

    #[test]
    fn warning_pay_as_you_go_above_threshold_silent() {
        // $20 left (above the $10 threshold) gets no warning
        let b = pay_as_you_go(100.0, 5000, 3000);
        assert_eq!(usage_warning(&b, None, true), None);
    }

    #[test]
    fn warning_pay_as_you_go_suppressed_below_full_usage() {
        // Pay-as-you-go users get no percentage warning before the included allowance is exhausted, even with low on-demand room remaining
        let b = pay_as_you_go(95.0, 5000, 4800);
        assert_eq!(usage_warning(&b, None, true), None);
    }

    #[test]
    fn warning_pay_as_you_go_fractional_dollars() {
        // $50 cap, $46.50 used leaves $3.50: critical, fractional formatting
        let b = pay_as_you_go(100.0, 5000, 4650);
        assert_eq!(
            usage_warning(&b, None, true),
            Some(("Pay-as-you-go limit left: $3.50".to_string(), true))
        );
    }

    #[test]
    fn test_credit_bar_line_shows_percentage() {
        let theme = Theme::default();
        let line = credit_bar_line(&bal(24.0), false, &theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Credits used: 24%");
    }

    #[test]
    fn test_color_thresholds() {
        let theme = Theme::default();

        let low = credit_bar_line(&bal(50.0), false, &theme);
        assert_eq!(low.spans[0].style.fg, Some(theme.accent_success));

        let high = credit_bar_line(&bal(85.0), false, &theme);
        assert_eq!(high.spans[0].style.fg, Some(theme.warning));

        let over = credit_bar_line(&bal(100.0), false, &theme);
        assert_eq!(over.spans[0].style.fg, Some(theme.accent_error));
    }

    #[test]
    fn test_zero_percent() {
        let theme = Theme::default();
        let line = credit_bar_line(&bal(0.0), false, &theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Credits used: 0%");
        assert_eq!(line.spans[0].style.fg, Some(theme.accent_success));
    }

    #[test]
    fn test_boundary_at_80_percent() {
        let theme = Theme::default();
        // Exactly 80% renders yellow (warning)
        let at_80 = credit_bar_line(&bal(80.0), false, &theme);
        assert_eq!(at_80.spans[0].style.fg, Some(theme.warning));

        // Just below 80% renders green (success)
        let below_80 = credit_bar_line(&bal(79.9), false, &theme);
        assert_eq!(below_80.spans[0].style.fg, Some(theme.accent_success));
    }

    #[test]
    fn test_boundary_at_100_percent() {
        let theme = Theme::default();
        // Exactly 100% renders red (error)
        let at_100 = credit_bar_line(&bal(100.0), false, &theme);
        assert_eq!(at_100.spans[0].style.fg, Some(theme.accent_error));

        // Just below 100% renders yellow (warning)
        let below_100 = credit_bar_line(&bal(99.9), false, &theme);
        assert_eq!(below_100.spans[0].style.fg, Some(theme.warning));
    }

    #[test]
    fn test_over_100_percent() {
        let theme = Theme::default();
        let line = credit_bar_line(&bal(150.0), false, &theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Credits used: 150%");
        assert_eq!(line.spans[0].style.fg, Some(theme.accent_error));
    }

    #[test]
    fn test_fractional_percentage_rounds_display() {
        let theme = Theme::default();
        let line = credit_bar_line(&bal(33.7), false, &theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Credits used: 34%");
    }

    #[test]
    fn test_credit_balance_with_on_demand_fields() {
        let balance = CreditBalance {
            effective_usage_pct: 25.0,
            period_end_display: Some("Jun 1, 00:00".into()),
            pay_as_you_go: true,
            on_demand_cap_cents: Some(2000),
            on_demand_used_cents: Some(500),
            ..bal(50.0)
        };
        let theme = Theme::default();
        // The credit bar uses usage_pct (not effective_usage_pct).
        let line = credit_bar_line(&balance, false, &theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Credits used: 50%");
    }

    // ── format_quota_chip / format_reset_remaining ────────────────────

    #[test]
    fn format_reset_remaining_units() {
        assert_eq!(format_reset_remaining(4 * 86_400 + 5 * 3_600), "4d5h");
        assert_eq!(format_reset_remaining(4 * 86_400), "4d");
        assert_eq!(format_reset_remaining(5 * 3_600 + 12 * 60), "5h12m");
        assert_eq!(format_reset_remaining(3_600), "1h");
        assert_eq!(format_reset_remaining(45 * 60), "45m");
        assert_eq!(format_reset_remaining(30), "<1m");
        assert_eq!(format_reset_remaining(0), "<1m");
    }

    #[test]
    fn format_quota_chip_with_and_without_period_end() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let end = now + chrono::Duration::days(4) + chrono::Duration::hours(5);
        let with_end = CreditBalance {
            period_end: Some(end),
            ..bal(10.4)
        };
        assert_eq!(
            format_quota_chip_at(&with_end, now),
            "10% (reset: 4d5h)"
        );
        assert_eq!(format_quota_chip_at(&bal(42.9), now), "42%");
    }

    #[test]
    fn quota_chip_for_prompt_shows_refreshing_while_fetch_in_flight() {
        let balance = bal(10.0);
        assert_eq!(
            quota_chip_for_prompt(true, false, true, Some(&balance)).as_deref(),
            Some("refreshing..."),
            "in-flight fetch must replace the cached chip"
        );
        assert_eq!(
            quota_chip_for_prompt(true, false, true, None).as_deref(),
            Some("refreshing..."),
            "in-flight fetch shows even when no balance is cached yet"
        );
        // Idle + cached balance → normal chip (not "refreshing...").
        let idle = quota_chip_for_prompt(true, false, false, Some(&balance));
        assert!(
            idle.as_deref().is_some_and(|s| s.starts_with("10%")),
            "idle chip should be the cached balance, got {idle:?}"
        );
        assert_eq!(
            quota_chip_for_prompt(true, false, false, None),
            None,
            "idle with no cache shows nothing"
        );
        assert_eq!(
            quota_chip_for_prompt(false, false, true, Some(&balance)),
            None,
            "hidden billing surface never shows the chip"
        );
        assert_eq!(
            quota_chip_for_prompt(true, true, true, Some(&balance)),
            None,
            "gateway chat suppresses the quota chip"
        );
    }

    #[test]
    fn prompt_border_chips_order_context_mode_quota() {
        use crate::views::prompt_widget::PromptFlag;
        let theme = Theme::default();
        let balance = bal(10.0);
        let chips = PromptBorderChips::for_prompt(
            false,
            Some(47_000),
            Some(500_000),
            &theme,
            true,
            false,
            Some(&balance),
        );
        assert!(chips.context.is_some());
        assert!(chips.quota.as_deref().is_some_and(|s| s.starts_with("10%")));

        let mode = vec![
            PromptFlag {
                text: "always-approve",
                color: None,
                bold: false,
            },
            PromptFlag {
                text: "plan",
                color: None,
                bold: false,
            },
        ];
        let woven = chips.with_mode_flags(mode);
        assert_eq!(woven.len(), 3);
        assert!(woven[0].text.contains('/'), "context first: {}", woven[0].text);
        assert_eq!(woven[1].text, "plan");
        assert!(woven[2].text.starts_with("10%"), "quota last: {}", woven[2].text);
        assert!(
            woven.iter().all(|f| f.text != "always-approve"),
            "always-approve must be filtered; flags: {:?}",
            woven.iter().map(|f| f.text).collect::<Vec<_>>()
        );

        let only = chips.chip_only_flags();
        assert_eq!(only.len(), 2);
        assert!(only[0].text.contains('/'));
        assert!(only[1].text.starts_with("10%"));
    }

    #[test]
    fn gateway_chat_suppresses_credit_bar_and_usage_warning() {
        let theme = Theme::default();
        let b = bal(90.0);
        assert!(credit_bar_line_for_session(&b, false, &theme, true).is_none());
        assert!(usage_warning_for_session(&b, None, true, true).is_none());
        // Build path still renders.
        assert!(credit_bar_line_for_session(&b, false, &theme, false).is_some());
    }
}
