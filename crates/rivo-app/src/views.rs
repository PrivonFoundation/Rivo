//! Rivo's desktop views. Layout follows a friendly chat-app shape: a narrow icon rail,
//! a project-grouped session sidebar, a tab strip, a floating composer and a right inspector.

use std::fmt::Write;

use rivo_agent::{
    KILO_FREE_MODELS, Message, PermissionMode, Preset, Protocol, Role, ToolCall, info,
    catalog::Category,
    registry::InstallSpec,
};
use rune::{Color, El, Icon, col, el, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown, Placement,
    Popover, ScrollBar, ScrollBarStyle, TextArea, TextInput,
};

use crate::{Rivo, InspectorTab, PROJECTS, SettingsTab};

// ----------------------------------------------------------------------------- icons

static PLUS: Icon = Icon {
    name: "rivo-plus", view: 16.0,
    body: r#"<path d="M8 3V13M3 8H13" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};
static SEND: Icon = Icon {
    name: "rivo-send", view: 16.0,
    body: r#"<path d="M8 13V3M4 7L8 3L12 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static STOP: Icon = Icon {
    name: "rivo-stop", view: 16.0,
    body: r#"<rect x="4.5" y="4.5" width="7" height="7" rx="1.5" fill="currentColor"/>"#,
};
static CHECK: Icon = Icon {
    name: "rivo-check", view: 16.0,
    body: r#"<path d="M3 8.5L6.5 12L13 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static RUNNING: Icon = Icon {
    name: "rivo-running", view: 16.0,
    body: r#"<circle cx="8" cy="8" r="3.5" fill="currentColor"/>"#,
};
static SETTINGS: Icon = Icon {
    name: "rivo-settings", view: 16.0,
    body: r#"<circle cx="8" cy="8" r="2.5" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M8 1.5V3.2M8 12.8V14.5M14.5 8H12.8M3.2 8H1.5M12.6 3.4L11.4 4.6M4.6 11.4L3.4 12.6M12.6 12.6L11.4 11.4M4.6 4.6L3.4 3.4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
};
/// The Rivo mark, exported from VectorCraft (foundation-style geometric R).
static RIVO_MARK: Icon = Icon {
    name: "rivo-mark",
    view: 240.0,
    body: r#"<path fill="currentColor" fill-rule="evenodd" d="M14.4 240 L0 0 L28.8 0 L45.6 0 L165.6 0 C206.4 0 240 33.6 240 74.4 C240 115.2 206.4 148.8 165.6 148.8 L157.67 148.8 L230.4 230.4 L187.2 240 L105.913 148.8 L40.527 148.8 L38.4 211.2 Z M45.6 50.4 L45.6 115.2 L165.6 115.2 C192 115.2 206.4 100.8 206.4 81.6 C206.4 62.4 192 50.4 165.6 50.4 Z"/>"#,
};

static BACK: Icon = Icon {
    name: "rivo-back", view: 16.0,
    body: r#"<path d="M9.5 3.5L5 8L9.5 12.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static SEARCH: Icon = Icon {
    name: "rivo-search", view: 16.0,
    body: r#"<circle cx="7" cy="7" r="3.75" stroke="currentColor" stroke-width="1.2" fill="none"/><path d="M10 10L12.75 12.75" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
};
static HOME: Icon = Icon {
    name: "rivo-home", view: 16.0,
    body: r#"<path d="M3 7.5L8 3L13 7.5V13H9.5V9.5H6.5V13H3Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#,
};
static GRID: Icon = Icon {
    name: "rivo-grid", view: 16.0,
    body: r#"<rect x="3" y="3" width="4" height="4" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/><rect x="9" y="3" width="4" height="4" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/><rect x="3" y="9" width="4" height="4" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/><rect x="9" y="9" width="4" height="4" rx="1" stroke="currentColor" stroke-width="1.2" fill="none"/>"#,
};
static CHEVRON: Icon = Icon {
    name: "rivo-chevron", view: 16.0,
    body: r#"<path d="M6 4L11 8L6 12" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static CHEVRON_DOWN: Icon = Icon {
    name: "rivo-chevron-down", view: 16.0,
    body: r#"<path d="M4 6L8 11L12 6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};
static CLOSE: Icon = Icon {
    name: "rivo-close", view: 16.0,
    body: r#"<path d="M5 5L11 11M11 5L5 11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
};
static STORE: Icon = Icon {
    name: "rivo-store", view: 16.0,
    body: r#"<path d="M2.5 6.5L3.5 2.5H12.5L13.5 6.5M2.5 6.5V13H13.5V6.5M2.5 6.5H13.5M6 13V9.5H10V13" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>"#,
};
static BADGE: Icon = Icon {
    name: "rivo-badge", view: 16.0,
    body: r#"<path d="M8 2L10 3.6L12.8 3.2L13.6 6L15.5 8L13.6 10L12.8 12.8L10 12.4L8 14L6 12.4L3.2 12.8L2.4 10L0.5 8L2.4 6L3.2 3.2L6 3.6Z" stroke="currentColor" stroke-width="1.1" fill="none"/><path d="M5.5 8L7.3 9.7L10.6 6.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" fill="none"/>"#,
};

// ------------------------------------------------------------------------- left rail

pub fn rail(s: &Rivo, k: &Kit, inset: f32) -> El {
    let c = &k.c;
    let item = |ic: &'static Icon, active: bool, label: &str, press: fn(&mut Rivo)| -> El {
        let mut b = row()
            .size(34.0, 34.0)
            .radius(k.radius.md)
            .center()
            .on_press(press)
            .label(label);
        if active { b = b.fill(c.layer_03); } else { b = b.hover(c.hover); }
        b.child(icon(ic, 17.0).color(if active { c.text_base } else { c.icon_muted }))
    };
    col()
        .key("rail")
        .w(48.0)
        .h_full()
        .padding(7.0)
        .gap(6.0)
        .fill(c.bg_base)
        .border_sides([0.0, 0.0, 0.0, 1.0], c.border_weak)
        .child(el().h(inset))
        .child(item(&HOME, true, "Sessions", |s| s.close_settings()))
        .child(item(&GRID, false, "Tools", |s: &mut Rivo| { s.settings_tab = SettingsTab::Tools; s.open_settings(); }))
        .child(item(&SETTINGS, s.settings_open, "Settings", |s: &mut Rivo| {
            if s.settings_open { s.close_settings(); } else { s.open_settings(); }
        }))
        .child(col().grow(1.0))
        .child(
            row().size(28.0, 28.0).radius(14.0).center().fill(Color::BLACK)
                .border(1.0, c.border_weak)
                .child(icon(&RIVO_MARK, 14.0).color(Color::WHITE))
                .help("Rivo"),
        )
}

// ---------------------------------------------------------------------------- sidebar

fn session_row(s: &Rivo, k: &Kit, id: &str) -> El {
    let c = &k.c;
    let Some(session) = s.sessions.iter().find(|x| x.id == id) else { return el() };
    let active = s.selected == id;
    let mut r = row()
        .key(format!("nav-{id}"))
        .w_full()
        .min_h(33.0)
        .padding(9.0)
        .radius(k.radius.sm)
        .gap(8.0)
        .center_y()
        .on_press({
            let id = id.to_string();
            move |s: &mut Rivo| s.select(&id)
        });
    if active { r = r.fill(c.layer_03); } else { r = r.hover(c.hover); }
    r = r.child(
        col().grow(1.0).min_w(0.0).gap(1.0)
            .child(text(session.title.clone()).font(k.ty.ui_compact).color(if active { c.text_base } else { c.text_muted }).truncate())
            .child(text(&session.project).font(k.ty.small).color(c.text_faint)),
    );
    if session.busy {
        r = r.child(icon(&RUNNING, 11.0).color(c.text_accent));
    }
    r.child(
        row().size(18.0, 18.0).radius(4.0).center().hover(c.hover)
            .on_press({
                let id = id.to_string();
                move |s: &mut Rivo| s.close_session(&id)
            })
            .child(icon(&CLOSE, 11.0).color(c.icon_muted)),
    )
}

pub fn sidebar(s: &Rivo, k: &Kit) -> El {
    let c = &k.c;
    let mut projects: Vec<(&str, Vec<&str>)> = Vec::new();
    for session in &s.sessions {
        if let Some((_, rows)) = projects.iter_mut().find(|(name, _)| *name == session.project) {
            rows.push(&session.id);
        } else {
            projects.push((&session.project, vec![&session.id]));
        }
    }

    let mut list = col().key("session-list").w_full().min_h(0.0).gap(10.0).scroll(rune::ScrollSpec { stick_bottom: false });
    list = list.child(text("Projects").font(k.ty.small_medium).color(c.text_faint).pl(4.0));
    for (project, ids) in projects {
        let closed = s.closed_projects.iter().any(|p| p == project);
        let (name, _) = PROJECTS.iter().find(|(n, _)| *n == project).copied().unwrap_or((project, 250.0));
        list = list.child(
            row().key(format!("project-{name}")).w_full().gap(7.0).center_y()
                .on_press({
                    let name = name.to_string();
                    move |s: &mut Rivo| s.toggle_project(&name)
                })
                .child(icon(&CHEVRON, 12.0).color(c.icon_faint).rotate(if closed { 0.0 } else { std::f32::consts::PI / 2.0 }))
                .child(row().size(14.0, 14.0).radius(4.0).center().fill(Color::BLACK)
                    .border(1.0, c.border_weak)
                    .child(text(name.chars().next().unwrap_or('?').to_ascii_uppercase().to_string()).font(k.ty.small).color(Color::WHITE)))
                .child(text(name).font(k.ty.ui_medium).color(c.text_base))
                .child(text(ids.len().to_string()).font(k.ty.small).color(c.text_faint))
        );
        if !closed {
            for id in ids {
                list = list.child(session_row(s, k, id).pl(18.0));
            }
        }
    }

    col()
        .key("sidebar")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(14.0)
        .gap(12.0)
        .fill(c.layer_01)
        .child(
            row().w_full().center_y().justify_between()
                .child(text("Rivo").font(rune::tok::type_::TITLE).color(c.text_base))
                .child(IconButton::new(&SEARCH).variant(IconButtonVariant::GhostMuted).help("Search sessions")
                    .on_press(|s: &mut Rivo| { s.settings_tab = SettingsTab::Provider; s.open_settings(); }).build(k)),
        )
        .child(
            row().w_full().min_h(34.0).padding(10.0).radius(k.radius.sm)
                .fill(c.layer_02).hover(c.hover).gap(8.0).center_y()
                .on_press(|s: &mut Rivo| s.new_session())
                .child(icon(&PLUS, 14.0).color(c.text_muted))
                .child(text("New session").font(k.ty.ui).color(c.text_base)),
        )
        .child(list)
        .child(
            row().w_full().min_h(30.0).padding(8.0).radius(k.radius.sm).gap(8.0).center_y()
                .hover(c.hover).on_press(|s: &mut Rivo| s.open_settings())
                .child(icon(&SETTINGS, 14.0).color(c.icon_muted))
                .child(text("Settings").font(k.ty.ui_compact).color(c.text_muted)),
        )
}

// ---------------------------------------------------------------------------- tab bar

pub fn tab_bar(s: &Rivo, k: &Kit, inset: f32) -> El {
    let c = &k.c;
    let mut tabs = row().key("tabs").grow(1.0).min_w(0.0).gap(4.0).scroll_x().center_y();
    for id in &s.open_tabs {
        let Some(session) = s.sessions.iter().find(|x| &x.id == id) else { continue };
        let active = &s.selected == id;
        let mut tab = row()
            .key(format!("tab-{id}"))
            .min_h(28.0)
            .padding(9.0)
            .radius(k.radius.sm)
            .gap(8.0)
            .center_y()
            .shrink()
            .on_press({
                let id = id.clone();
                move |s: &mut Rivo| s.select(&id)
            });
        if active { tab = tab.fill(c.layer_03); } else { tab = tab.hover(c.hover); }
        tab = tab.child(text(session.title.clone()).font(k.ty.ui_compact).color(if active { c.text_base } else { c.text_muted }).truncate());
        let id2 = id.clone();
        tab = tab.child(
            row().size(15.0, 15.0).radius(4.0).center().hover(c.hover)
                .on_press(move |s: &mut Rivo| s.close_tab(&id2))
                .child(icon(&CLOSE, 10.0).color(c.icon_faint)),
        );
        tabs = tabs.child(tab);
    }
    col().key("tabstrip").w_full().pt(inset).padding(7.0).gap(6.0)
        .fill(if k.dark { Color::BLACK } else { Color::WHITE }).border_sides([0.0, 0.0, 1.0, 0.0], c.border_weak)
        .child(
            row().w_full().center_y()
                .child(tabs)
                .child(IconButton::new(&PLUS).variant(IconButtonVariant::GhostMuted).help("New session")
                    .on_press(|s: &mut Rivo| s.new_session()).build(k))
                .child(IconButton::new(&rune_kit::shell::INSPECTOR).variant(IconButtonVariant::GhostMuted)
                    .expanded(s.inspector.is_open()).help("Toggle inspector")
                    .on_press(|s: &mut Rivo| s.inspector.toggle()).build(k)),
        )
}

// --------------------------------------------------------------------------- transcript

fn tool_card(call: &ToolCall, k: &Kit) -> El {
    let c = &k.c;
    let (tone, status) = if !call.finished {
        (c.text_accent, "running")
    } else if call.ok {
        (c.success.fg, "done")
    } else {
        (c.danger.fg, "failed")
    };
    let args: String = match serde_json::from_str::<serde_json::Value>(&call.arguments) {
        Ok(v) => {
            let mut out = String::new();
            fmt_json(&v, 0, &mut out);
            out
        }
        Err(_) => call.arguments.clone(),
    };
    let header = row().gap(7.0).center_y()
        .child(if call.finished { icon(&CHECK, 13.0) } else { icon(&RUNNING, 13.0) }.color(tone))
        .child(text(call.name.clone()).font(k.ty.ui_medium).color(c.text_base))
        .child(text(status).font(k.ty.small).color(tone))
        .grow(1.0);
    let mut body = col().key(format!("tool-{}", call.id)).w_full().radius(k.radius.md)
        .border(1.0, c.border_weak).fill(c.layer_01).padding(10.0).gap(7.0)
        .child(header)
        .child(text(args).font(k.ty.mono).color(c.text_muted).wrap().selectable(true));
    if let Some(result) = &call.result {
        body = body.child(
            el().key(format!("tool-result-{}", call.id)).w_full().max_h(130.0)
                .padding(8.0).radius(k.radius.sm).fill(c.layer_02)
                .scroll(rune::ScrollSpec { stick_bottom: true })
                .child(text(result.clone()).font(k.ty.mono).color(if call.ok { c.text_base } else { c.danger.fg }).wrap().selectable(true)),
        );
    }
    body
}

fn fmt_json(v: &serde_json::Value, depth: usize, out: &mut String) {
    match v {
        serde_json::Value::Object(map) => {
            out.push('{');
            for (i, (key, val)) in map.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push('\n');
                let _ = write!(out, "{}{key}: ", "  ".repeat(depth + 1));
                fmt_json(val, depth + 1, out);
            }
            out.push('\n');
            let _ = write!(out, "{}}}", "  ".repeat(depth));
        }
        serde_json::Value::Array(a) => {
            out.push('[');
            for (i, val) in a.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push('\n');
                let _ = write!(out, "{}", "  ".repeat(depth + 1));
                fmt_json(val, depth + 1, out);
            }
            out.push('\n');
            let _ = write!(out, "{}]", "  ".repeat(depth));
        }
        other => {
            let s = other.to_string();
            let short: String = s.chars().take(220).collect();
            let _ = write!(out, "{short}{}", if s.chars().count() > 220 { "…" } else { "" });
        }
    }
}

enum Segment {
    Prose(String),
    Code { lang: Option<String>, code: String },
    Diff(String),
}

fn segments(text: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut prose = String::new();
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let fence = if trimmed.starts_with("```") { "```" } else if trimmed.starts_with("~~~") { "~~~" } else { "" };
        if fence.is_empty() {
            prose.push_str(line);
            prose.push('\n');
            continue;
        }
        if !prose.trim().is_empty() {
            out.push(Segment::Prose(std::mem::take(&mut prose)));
        }
        let info = trimmed[fence.len()..].trim();
        let lang = info.split_whitespace().next().map(str::to_string);
        let mut code = String::new();
        while let Some(next) = lines.next() {
            if next.trim_start().starts_with(fence) {
                break;
            }
            code.push_str(next);
            code.push('\n');
        }
        if lang.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("diff") || l.eq_ignore_ascii_case("patch")) {
            out.push(Segment::Diff(code));
        } else {
            out.push(Segment::Code { lang, code });
        }
    }
    if !prose.trim().is_empty() {
        out.push(Segment::Prose(prose));
    }
    out
}

fn code_block(lang: Option<&str>, code: &str, k: &Kit, key: &str) -> El {
    let c = &k.c;
    let mut frame = col().key(key.to_string()).w_full().max_w(760.0)
        .radius(k.radius.md).border(1.0, c.border_weak).fill(c.layer_02)
        .padding(12.0).gap(8.0);
    if let Some(lang) = lang.filter(|l| !l.is_empty()) {
        frame = frame.child(row().w_full().justify_between().center_y()
            .child(text(lang.to_string()).font(k.ty.small_medium).color(c.text_muted))
            .child(text(format!("{} lines", code.lines().count())).font(k.ty.small).color(c.text_faint)));
    }
    let mut lines = col().w_full();
    for (n, line) in code.lines().enumerate() {
        let shown = if line.is_empty() { " " } else { line };
        lines = lines.child(row().w_full()
            .child(row().w(34.0).justify_end().pr(8.0)
                .child(text((n + 1).to_string()).font(k.ty.small).color(c.text_faint)))
            .child(text(shown).font(k.ty.mono).color(c.text_base).wrap().w_full()));
    }
    frame.child(lines)
}

fn parse_hunk(line: &str) -> Option<(usize, usize)> {
    let old = line.split('-').nth(1)?
        .split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty())?.parse().ok()?;
    let new = line.split('+').nth(1)?
        .split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty())?.parse().ok()?;
    Some((old, new))
}

fn diff_block(diff: &str, k: &Kit, key: &str) -> El {
    let c = &k.c;
    let mut old_no = 1usize;
    let mut new_no = 1usize;
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut rows = col().w_full();
    for (n, line) in diff.lines().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with("@@") {
            if let Some((old, new)) = parse_hunk(line) {
                old_no = old;
                new_no = new;
            }
            rows = rows.child(row().key(format!("{key}-h{n}")).w_full().px(8.0).py(3.0)
                .child(text(line).font(k.ty.small).color(c.text_muted)));
            continue;
        }
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("diff ") || line.starts_with("index ") {
            rows = rows.child(row().key(format!("{key}-h{n}")).w_full().px(8.0).py(3.0)
                .child(text(line).font(k.ty.small).color(c.text_muted)));
            continue;
        }
        let (old_label, new_label, tint) = match line.as_bytes().first() {
            Some(b'+') => {
                added += 1;
                let label = new_no.to_string();
                new_no += 1;
                (String::new(), label, Some(c.success.fg.alpha(0.14)))
            }
            Some(b'-') => {
                removed += 1;
                let label = old_no.to_string();
                old_no += 1;
                (label, String::new(), Some(c.danger.fg.alpha(0.14)))
            }
            Some(b'\\') => (String::new(), String::new(), None),
            _ => {
                let old = old_no.to_string();
                let new = new_no.to_string();
                old_no += 1;
                new_no += 1;
                (old, new, None)
            }
        };
        let shown = if line.is_empty() { " " } else { line };
        let mut r = row().key(format!("{key}-l{n}")).w_full().px(6.0).py(1.0);
        if let Some(tint) = tint {
            r = r.fill(tint);
        }
        r = r.child(row().w(34.0).justify_end().pr(6.0)
            .child(text(old_label).font(k.ty.small).color(c.text_faint)));
        r = r.child(row().w(34.0).justify_end().pr(6.0)
            .child(text(new_label).font(k.ty.small).color(c.text_faint)));
        r = r.child(text(shown).font(k.ty.mono).color(c.text_base).wrap().w_full());
        rows = rows.child(r);
    }
    let summary = row().w_full().justify_between().center_y()
        .child(text("Diff").font(k.ty.small_medium).color(c.text_muted))
        .child(row().gap(8.0)
            .child(text(format!("+{added}")).font(k.ty.small_medium).color(c.success.fg))
            .child(text(format!("−{removed}")).font(k.ty.small_medium).color(c.danger.fg)));
    col().key(key.to_string()).w_full().max_w(760.0)
        .radius(k.radius.md).border(1.0, c.border_weak).fill(c.layer_02)
        .padding(10.0).gap(6.0)
        .child(summary)
        .child(rows)
}

fn message(m: &Message, k: &Kit, streaming: bool, i: usize) -> El {
    let c = &k.c;
    if m.role != Role::User {
        let mut out = col().key(format!("msg-{i}")).w_full().max_w(760.0).gap(12.0);
        if !m.text.is_empty() {
            for (j, segment) in segments(&m.text).into_iter().enumerate() {
                let key = format!("msg-{i}-{j}");
                out = out.child(match segment {
                    Segment::Prose(text) => el().key(key).child(Markdown::new(text)
                        .streaming(streaming).reveal(if streaming { 0.12 } else { 0.0 }).selectable(true).build(k)),
                    Segment::Code { lang, code } => code_block(lang.as_deref(), &code, k, &key),
                    Segment::Diff(diff) => diff_block(&diff, k, &key),
                });
            }
        }
        for call in &m.tools { out = out.child(tool_card(call, k)); }
        return out;
    }

    let mut card = col().key(format!("msg-{i}")).w_full().max_w(760.0)
        .radius(k.radius.lg).border(1.0, c.border_weak)
        .fill(c.layer_02).padding(14.0).gap(10.0)
        .child(text("You").font(k.ty.small_medium).color(c.text_faint));
    if !m.text.is_empty() {
        card = card.child(text(m.text.clone()).font(k.ty.body).color(c.text_base).wrap().selectable(true));
    }
    for call in &m.tools { card = card.child(tool_card(call, k)); }
    card
}

pub fn transcript(s: &Rivo, k: &Kit) -> El {
    let session = s.current();
    let mut list = col().key("transcript-list").w_full().h_full().min_h(0.0)
        .padding(22.0).gap(14.0)
        .scroll(rune::ScrollSpec { stick_bottom: true }).scroll_edge_fade(14.0);
    for (i, m) in session.messages.iter().enumerate() {
        let streaming = s.busy() && i + 1 == session.messages.len() && m.role == Role::Assistant;
        list = list.child(row().w_full().center().child(message(m, k, streaming, i)));
    }
    if let Some(removed) = s.compaction_notice {
        list = list.child(text(format!("Context compacted — {removed} older messages folded."))
            .font(k.ty.small).color(k.c.text_faint));
    }
    k.scroll_bar(list)
}

pub fn empty_state(s: &Rivo, k: &Kit) -> El {
    let c = &k.c;
    let chip = |label: &str, prompt: &str| {
        let prompt = prompt.to_string();
        row().min_h(32.0).padding(10.0).radius(k.radius.md).fill(c.layer_02)
            .border(1.0, c.border_weak).hover(c.hover).center_y()
            .on_press(move |s: &mut Rivo| { s.draft = rune::TextEdit::new(prompt.clone()); })
            .child(text(label.to_owned()).font(k.ty.ui_compact).color(c.text_base))
    };
    col().key("empty-state").w_full().h_full().center().gap(18.0)
        .child(row().size(44.0, 44.0).radius(12.0).center().fill(Color::BLACK)
            .border(1.0, c.border_weak)
            .child(icon(&RIVO_MARK, 30.0).color(Color::WHITE)))
        .child(text("What are we building?").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(text("Rivo can read, edit and run code in your workspace — every tool call is shown below.")
            .font(k.ty.body).color(c.text_muted))
        .child(row().gap(8.0).center()
            .child(chip("List the files", "list the files in this workspace"))
            .child(chip("Review the code", "load the code-review skill and review this project"))
            .child(chip("Write a test", "write a test for the main module")))
        .child(text(format!("{} tools · {} skills · {}", rivo_agent::tools::all_tools().len(), s.skills.len(), s.config.label()))
            .font(k.ty.small).color(c.text_faint))
}

// ----------------------------------------------------------------------------- composer

fn format_tokens(n: usize) -> String {
    if n >= 1_000_000 {
        let m = n as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("{}M", m.round() as usize)
        } else {
            format!("{m:.1}M")
        }
    } else if n >= 1_000 {
        let k = n as f64 / 1_000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{}k", k.round() as usize)
        } else {
            format!("{k:.1}k")
        }
    } else {
        n.to_string()
    }
}

pub fn composer(s: &Rivo, k: &Kit, layer: &mut Vec<El>) -> El {
    let c = &k.c;
    let busy = s.busy();
    let send = if busy {
        IconButton::new(&STOP).variant(IconButtonVariant::GhostMuted).control_size(ControlSize::Large).help("Stop")
            .on_press(|s: &mut Rivo| s.stop()).build(k)
    } else {
        IconButton::new(&SEND).variant(IconButtonVariant::Submit).control_size(ControlSize::Large).help("Send message")
            .on_press(|s: &mut Rivo| s.submit()).build(k)
    };
    let usage = format!("~{} / {} context", format_tokens(s.context_used), format_tokens(s.context_window));
    let current_model = KILO_FREE_MODELS
        .iter()
        .find(|m| m.id == s.config.model)
        .map(|m| m.name.to_string())
        .or_else(|| s.catalog_models.iter().find(|m| m.id == s.config.model).map(|m| m.name.clone()))
        .unwrap_or_else(|| s.config.model.clone());
    let current_model = current_model.as_str();
    let model_trigger = Button::new(format!("{current_model}  ⌄"))
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .help("Switch model")
        .on_press(|s: &mut Rivo| s.toggle_model_picker())
        .build(k);

    let query = s.model_search.text.trim().to_lowercase();
    let filtered: Vec<_> = KILO_FREE_MODELS
        .iter()
        .filter(|m| query.is_empty() || m.name.to_lowercase().contains(&query) || m.id.to_lowercase().contains(&query))
        .collect();
    let mut options = col().key("model-options").w_full().max_h(220.0).min_h(0.0)
        .scroll(rune::ScrollSpec { stick_bottom: false }).scroll_edge_fade(8.0);
    for m in &filtered {
        let id = m.id;
        let selected = s.config.model == id;
        let mut row = row().w_full().min_h(28.0).padding(6.0).radius(k.radius.sm).gap(8.0).center_y()
            .on_press(move |s: &mut Rivo| s.choose_model(id));
        if selected { row = row.fill(k.c.hover); } else { row = row.hover(k.c.hover); }
        row = row.child(col().gap(1.0).grow(1.0)
            .child(text(m.name).font(k.ty.ui_compact).color(k.c.text_base))
            .child(text(format!("{} · {}", m.id, m.context)).font(k.ty.small).color(k.c.text_muted)));
        if selected { row = row.child(icon(&CHECK, 14.0).color(k.c.text_accent)); }
        options = options.child(row);
    }
    if filtered.is_empty() {
        options = options.child(text("No models match").font(k.ty.small).color(k.c.text_muted));
    }
    let options = options.scroll_bar_styled(
        ScrollBarStyle::from_kit(k).inside().thumb(k.c.text_muted).thumb_hover(k.c.text_base),
    );

    let search = TextInput::new(&s.model_search)
        .placeholder("Search models…")
        .label("Search models")
        .width(None)
        .leading_icon(k.glyph.search)
        .on_edit(|s: &mut Rivo| &mut s.model_search)
        .on_key(|s: &mut Rivo, ev, _| {
            if ev.key == rune::Key::Escape {
                s.close_model_picker();
                true
            } else {
                false
            }
        })
        .build(k);
    let body = col().key("model-picker-body").w_full().gap(6.0)
        .child(search)
        .child(el().key("model-picker-divider").w_full().h(1.0).fill(k.c.border_weak).a11y_hidden())
        .child(options);
    let model_picker = Popover::new("composer-model")
        .trigger(model_trigger)
        .open(s.model_picker_open)
        .placement(Placement::TopStart)
        .padding(10.0)
        .label("Model")
        .child(body)
        .on_close(|s: &mut Rivo| s.close_model_picker())
        .build(k, layer);
    col().key("composer").w_full().max_w(880.0)
        .radius(24.0).border(1.0, c.border_strong).fill(c.layer_02)
        .shadow(4.0, 22.0, Color::BLACK.alpha(0.18))
        .padding(14.0).gap(10.0)
        .child(TextArea::new(&s.draft)
            .placeholder("Type a message — / for commands, @ for skills or files…")
            .label("Message").width(None).rows(2).auto_grow(2, 6).max_height(160.0)
            .on_edit(|s: &mut Rivo| &mut s.draft)
            .on_submit(|s: &mut Rivo, _| s.submit()).build(k))
        .child(row().justify_between().center_y()
            .child(row().gap(8.0).center_y()
                .child(model_picker)
                .child(text(usage).font(k.ty.small).color(c.text_faint)))
            .child(row().gap(6.0).center_y()
                .child(IconButton::new(&PLUS).variant(IconButtonVariant::GhostMuted).control_size(ControlSize::Small).help("Skills")
                    .on_press(|s: &mut Rivo| { s.settings_tab = SettingsTab::Skills; s.open_settings(); }).build(k))
                .child(send)))
}

// ---------------------------------------------------------------------------- inspector

fn money(v: f64) -> String {
    if v.abs() < 0.005 && v.is_sign_negative() { "$0.00".into() } else { format!("${v:.2}") }
}

fn grouped(n: u64) -> String {
    let raw = n.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (i, ch) in raw.chars().enumerate() {
        if i > 0 && (raw.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn short_path(path: &str, max: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max {
        return path.to_string();
    }
    let tail: String = chars[chars.len() - (max - 1)..].iter().collect();
    format!("…{tail}")
}

fn model_label(s: &Rivo, name: &str) -> String {
    KILO_FREE_MODELS.iter().find(|m| m.id == name).map(|m| m.name.to_string())
        .or_else(|| s.catalog_models.iter().find(|m| m.id == name).map(|m| m.name.clone()))
        .unwrap_or_else(|| name.to_string())
}

fn metric_row(k: &Kit, label: &str, value: String) -> El {
    row().w_full().min_h(22.0).gap(8.0).center_y()
        .child(text(label.to_string()).font(k.ty.small).color(k.c.text_muted))
        .child(el().grow(1.0))
        .child(text(value).font(k.ty.mono).color(k.c.text_base).truncate())
}

fn section_header(s: &Rivo, k: &Kit, key: &str, title: &str, suffix: Option<String>) -> El {
    let c = &k.c;
    let open = s.info_open(key);
    let key = key.to_string();
    let mut h = row().key(format!("section-{key}")).w_full().min_h(28.0).padding(4.0)
        .radius(k.radius.sm).gap(6.0).center_y().hover(c.hover)
        .on_press(move |s: &mut Rivo| s.toggle_info_section(&key));
    h = h.child(icon(if open { &CHEVRON_DOWN } else { &CHEVRON }, 12.0).color(c.icon_muted))
        .child(text(title.to_string()).font(k.ty.small_medium).color(c.text_base));
    if let Some(suffix) = suffix {
        h = h.child(el().grow(1.0))
            .child(text(suffix).font(k.ty.small).color(c.text_muted).truncate());
    }
    h
}

fn tab_pill(s: &Rivo, k: &Kit, label: &str, tab: InspectorTab) -> El {
    let c = &k.c;
    let active = s.inspector_tab == tab;
    let mut b = row().padding(7.0).radius(k.radius.sm).center()
        .on_press(move |s: &mut Rivo| s.set_inspector_tab(tab));
    if active { b = b.fill(c.layer_03); } else { b = b.hover(c.hover); }
    b.child(text(label.to_string()).font(k.ty.small).color(if active { c.text_base } else { c.text_muted }))
}

pub fn inspector(s: &Rivo, k: &Kit) -> El {
    let c = &k.c;
    let session = s.current();
    let field = |label: &str, value: String| {
        col().w_full().gap(3.0)
            .child(text(label.to_owned()).font(k.ty.small).color(c.text_faint))
            .child(text(value).font(k.ty.ui).color(c.text_base).wrap().selectable(true))
    };
    let mut body = col().key("inspector").w_full().h_full().min_h(0.0).padding(16.0).gap(14.0)
        .fill(c.layer_01)
        .scroll(rune::ScrollSpec { stick_bottom: false });
    body = body.child(row().gap(3.0).w_full()
        .child(tab_pill(s, k, "Info", InspectorTab::Info))
        .child(tab_pill(s, k, "Details", InspectorTab::Details))
        .child(tab_pill(s, k, "Tools", InspectorTab::Tools))
        .child(tab_pill(s, k, "Skills", InspectorTab::Skills)));

    match s.inspector_tab {
        InspectorTab::Details => {
            body = body.child(text("Session").font(k.ty.ui_medium).color(c.text_base))
                .child(field("Title", session.title.clone()))
                .child(field("Workspace", session.cwd.display().to_string()))
                .child(field("Provider", s.config.label().to_string()))
                .child(field("Protocol", s.model_config.protocol.label().to_string()))
                .child(field("Model", s.config.model.clone()))
                .child(field("Messages", session.messages.len().to_string()))
                .child(Button::new("Open settings").variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Regular).on_press(|s: &mut Rivo| s.open_settings()).build(k));
        }
        InspectorTab::Tools => {
            body = body.child(text("Tools").font(k.ty.ui_medium).color(c.text_base))
                .child(text("Click to enable or disable a tool for the agent.").font(k.ty.small).color(c.text_muted).wrap());
            for tool in rivo_agent::tools::all_tools() {
                let on = s.agent_settings.tool_enabled(tool.name());
                let name = tool.name().to_string();
                let press_name = name.clone();
                let mut r = row().w_full().min_h(30.0).padding(7.0).radius(k.radius.sm)
                    .gap(8.0).center_y().hover(c.hover)
                    .on_press(move |s: &mut Rivo| s.toggle_tool(&press_name));
                r = r.child(icon(if on { &CHECK } else { &RUNNING }, 12.0).color(if on { c.success.fg } else { c.text_faint }))
                    .child(text(name).font(k.ty.mono).color(c.text_base).truncate());
                body = body.child(r);
            }
        }
        InspectorTab::Skills => {
            body = body.child(text("Skills").font(k.ty.ui_medium).color(c.text_base));
            for skill in &s.skills {
                let name = skill.name.clone();
                let mut r = row().w_full().min_h(42.0).padding(8.0).radius(k.radius.sm)
                    .gap(8.0).center_y().hover(c.hover)
                    .on_press(move |s: &mut Rivo| s.toggle_skill(&name));
                r = r.child(icon(&CHECK, 12.0).color(c.text_accent))
                    .child(col().gap(2.0).grow(1.0).min_w(0.0)
                        .child(text(skill.name.clone()).font(k.ty.ui_medium).color(c.text_base))
                        .child(text(skill.description.clone()).font(k.ty.small).color(c.text_muted).wrap()));
                body = body.child(r);
            }
        }
        InspectorTab::Info => {
            let pct = (s.context_used as f32 / s.context_window.max(1) as f32 * 100.0).min(100.0);
            let cost: f64 = s.model_stats.iter().map(|m| info::cost_usd(&m.name, &m.usage)).sum();
            let usage = &s.token_usage;

            body = body.child(section_header(s, k, "context", "Context", Some(format!("{pct:.0}% · {}", money(cost)))));
            if s.info_open("context") {
                body = body.child(field("Used", format!("~{} tokens", format_tokens(s.context_used))))
                    .child(field("Window", format!("{} tokens", format_tokens(s.context_window))))
                    .child(el().h(6.0).w_full().radius(3.0).fill(c.layer_03).child(
                        el().h(6.0).w(pct.max(1.0)).radius(3.0)
                            .fill(if pct > 85.0 { c.danger.fg } else { c.text_accent })));
                if s.agent_settings.auto_compact {
                    body = body.child(text(format!("Auto-compact keeps {} recent messages.", s.agent_settings.keep_recent))
                        .font(k.ty.small).color(c.text_muted).wrap());
                }
            }

            let total = usage.input + usage.output + usage.reasoning + usage.cache_read + usage.cache_write;
            body = body.child(section_header(s, k, "tokens", "Token Usage", Some(grouped(total))));
            if s.info_open("tokens") {
                body = body.child(metric_row(k, "Input", grouped(usage.input)))
                    .child(metric_row(k, "Output", grouped(usage.output)))
                    .child(metric_row(k, "Reasoning", grouped(usage.reasoning)))
                    .child(metric_row(k, "Cache read", grouped(usage.cache_read)))
                    .child(metric_row(k, "Cache write", grouped(usage.cache_write)))
                    .child(metric_row(k, "Cache rate", format!("{:.1}%", usage.cache_rate())))
                    .child(metric_row(k, "Cost", money(cost)));
            }

            body = body.child(section_header(s, k, "models", "Models", Some(format!("({})", s.model_stats.len()))));
            if s.info_open("models") {
                body = body.child(row().w_full().gap(4.0).padding(4.0)
                    .child(text("Model".to_string()).font(k.ty.small).color(c.text_faint))
                    .child(el().grow(1.0))
                    .child(text("Steps".to_string()).font(k.ty.small).color(c.text_faint).w(36.0))
                    .child(text("Cost".to_string()).font(k.ty.small).color(c.text_faint).w(46.0)));
                for stat in &s.model_stats {
                    let stat_cost = info::cost_usd(&stat.name, &stat.usage);
                    body = body.child(row().w_full().min_h(26.0).padding(4.0).gap(4.0).center_y().hover(c.hover)
                        .child(col().grow(1.0).min_w(0.0)
                            .child(text(model_label(s, &stat.name)).font(k.ty.ui_compact).color(c.text_base).truncate()))
                        .child(text(stat.steps.to_string()).font(k.ty.mono).color(c.text_muted).w(36.0))
                        .child(text(money(stat_cost)).font(k.ty.mono).color(c.text_muted).w(46.0)));
                }
                if s.model_stats.is_empty() {
                    body = body.child(text("No model calls in this session yet.").font(k.ty.small).color(c.text_muted));
                }
            }

            body = body.child(section_header(s, k, "mcp", "MCP", Some(format!("({})", s.mcp_servers.len()))));
            if s.info_open("mcp") {
                for server in &s.mcp_servers {
                    let ok = server.status == "Configured" || server.status == "Remote configured";
                    body = body.child(row().w_full().min_h(26.0).padding(4.0).gap(6.0).center_y()
                        .child(icon(&RUNNING, 10.0).color(if ok { c.success.fg } else { c.danger.fg }))
                        .child(col().grow(1.0).min_w(0.0)
                            .child(text(server.name.clone()).font(k.ty.ui_compact).color(c.text_base).truncate()))
                        .child(text(server.status.clone()).font(k.ty.small)
                            .color(if ok { c.text_muted } else { c.danger.fg }).truncate()));
                }
                if s.mcp_servers.is_empty() {
                    body = body.child(text("No MCP servers configured.").font(k.ty.small).color(c.text_muted));
                }
                body = body.child(text("Status reflects local config; Rivo does not host MCP transports yet.")
                    .font(k.ty.small).color(c.text_faint).wrap());
            }

            body = body.child(section_header(s, k, "indexing", "Code Indexing", Some("Disabled".into())));
            if s.info_open("indexing") {
                body = body.child(text("Code indexing is disabled.").font(k.ty.small).color(c.text_muted).wrap());
            }

            body = body.child(section_header(s, k, "lsp", "LSP", None));
            if s.info_open("lsp") {
                body = body.child(text("LSPs are disabled.").font(k.ty.small).color(c.text_muted).wrap());
            }

            body = body.child(section_header(s, k, "files", "Modified Files", Some(format!("({})", s.modified_files.len()))));
            if s.info_open("files") {
                for file in &s.modified_files {
                    body = body.child(row().w_full().min_h(26.0).padding(4.0).gap(6.0).center_y()
                        .child(col().grow(1.0).min_w(0.0)
                            .child(text(short_path(&file.path, 34)).font(k.ty.mono).color(c.text_base).truncate()))
                        .child(text(format!("+{}", file.added)).font(k.ty.mono).color(c.success.fg).w(38.0))
                        .child(text(format!("-{}", file.removed)).font(k.ty.mono).color(c.danger.fg).w(38.0)));
                }
                if s.modified_files.is_empty() {
                    body = body.child(text("Working tree is clean.").font(k.ty.small).color(c.text_muted));
                }
            }
        }
    }
    body
}

// ---------------------------------------------------------------------------- settings

fn settings_nav_item(s: &Rivo, k: &Kit, label: &str, tab: SettingsTab, ic: &'static Icon) -> El {
    let c = &k.c;
    let active = s.settings_tab == tab;
    let mut b = row().key(format!("settings-nav-{label}")).w_full().min_h(36.0)
        .padding(9.0).radius(k.radius.sm).gap(10.0).center_y()
        .on_press(move |s: &mut Rivo| s.choose_tab(tab));
    if active { b = b.fill(c.layer_03); } else { b = b.hover(c.hover); }
    b.child(icon(ic, 15.0).color(if active { c.text_base } else { c.icon_muted }))
        .child(text(label.to_string()).font(k.ty.ui_compact).color(if active { c.text_base } else { c.text_muted }))
}

fn settings_nav(s: &Rivo, k: &Kit) -> El {
    col().key("settings-nav").w(180.0).h_full().min_h(0.0).padding(10.0).gap(2.0)
        .fill(k.c.bg_deep)
        .border_sides([0.0, 1.0, 0.0, 0.0], k.c.border_weak)
        .child(text("SETTINGS").font(k.ty.small_medium).color(k.c.text_faint).pl(9.0).pb(6.0))
        .child(settings_nav_item(s, k, "General", SettingsTab::Provider, &SETTINGS))
        .child(settings_nav_item(s, k, "Providers", SettingsTab::Connection, &SEARCH))
        .child(settings_nav_item(s, k, "MCP Marketplace", SettingsTab::McpMarketplace, &STORE))
        .child(settings_nav_item(s, k, "Skills Marketplace", SettingsTab::SkillsMarketplace, &STORE))
        .child(settings_nav_item(s, k, "Skills", SettingsTab::Skills, &CHECK))
        .child(settings_nav_item(s, k, "Tools", SettingsTab::Tools, &PLUS))
        .child(settings_nav_item(s, k, "Permissions", SettingsTab::Permissions, &HOME))
}

fn card(k: &Kit, title: &str, rows: Vec<El>) -> El {
    let c = &k.c;
    let mut body = col().w_full().radius(k.radius.md).fill(c.layer_01).border(1.0, c.border_weak);
    body = body.child(row().w_full().padding(13.0).child(text(title.to_owned()).font(k.ty.ui_medium).color(c.text_base)));
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(el().h(1.0).w_full().fill(c.border_weak).pl(13.0));
        }
        body = body.child(r);
    }
    body
}

fn setting_row(k: &Kit, label: &str, desc: &str, control: El) -> El {
    let c = &k.c;
    row().w_full().min_h(56.0).padding(12.0).gap(14.0).center_y().justify_between()
        .child(col().grow(1.0).min_w(0.0).gap(3.0)
            .child(text(label.to_owned()).font(k.ty.ui).color(c.text_base))
            .child(text(desc.to_owned()).font(k.ty.small).color(c.text_muted).wrap()))
        .child(control)
}

fn select_box<F>(
    s: &Rivo,
    k: &Kit,
    layer: &mut Vec<El>,
    key: &str,
    options: &[String],
    current: &str,
    choose: F,
) -> El
where
    F: Fn(&mut Rivo, &str) + Clone + 'static,
{
    rune_kit::Select::new(key.to_string(), options)
        .current(Some(current))
        .open(s.settings_select_open.as_deref() == Some(key))
        .on_toggle({
            let key = key.to_string();
            move |s: &mut Rivo| s.toggle_settings_select(&key)
        })
        .on_close(|s: &mut Rivo| s.close_settings_select())
        .on_choose(move |s: &mut Rivo, v: &str, _: &mut rune::Tx| {
            s.close_settings_select();
            choose(s, v);
        })
        .width(214.0)
        .label(key)
        .build(k, layer)
}

fn toggle_switch<F: Fn(&mut Rivo) + 'static>(k: &Kit, on: bool, press: F) -> El {
    rune_kit::Switch::new(on).hide_label().label("Toggle setting").on_toggle(press).build(k)
}

fn general_panel(s: &Rivo, k: &Kit, layer: &mut Vec<El>) -> El {
    let c = &k.c;
    let permissions: Vec<String> = vec![PermissionMode::Auto.label().to_string(), PermissionMode::ReadOnly.label().to_string()];
    let permission = select_box(s, k, layer, "permission-select", &permissions, s.agent_settings.permission.label(), |s, v| {
        s.set_permission(if v.starts_with("Read") { PermissionMode::ReadOnly } else { PermissionMode::Auto });
    });

    let themes: Vec<String> = vec!["System".into(), "Dark".into(), "Light".into()];
    let theme_current = match s.theme_override { None => "System", Some(true) => "Dark", Some(false) => "Light" };
    let theme = select_box(s, k, layer, "theme-select", &themes, theme_current, |s, v| {
        s.theme_override = match v { "Dark" => Some(true), "Light" => Some(false), _ => None };
    });

    col().w_full().gap(18.0)
        .child(text("General").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(
            row().w_full().gap(8.0).center_y()
                .child(text("Applies to").font(k.ty.ui).color(c.text_muted))
                .child(select_box(s, k, layer, "scope-select", &["All sessions".into(), "Current session".into()], "All sessions", |_, _| {}))
                .child(text("in").font(k.ty.ui).color(c.text_muted))
                .child(select_box(s, k, layer, "env-select", &["All workspaces".into(), "Current workspace".into()], "All workspaces", |_, _| {})),
        )
        .child(card(k, "Agent defaults", vec![
            setting_row(k, "Permissions", "Read-only blocks writes and commands.", permission),
            setting_row(k, "Provider & model", "Endpoint, protocol, model, context and instructions are configured under Providers.",
                Button::new("Open Providers").variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut Rivo| s.choose_tab(SettingsTab::Connection)).build(k)),
        ]))
        .child(card(k, "Appearance", vec![
            setting_row(k, "Theme", "Follow the system or pick one appearance.", theme),
            setting_row(k, "Sidebar & inspector", "Both panels stay available; drag an edge to resize, ⌥⌘I to toggle the inspector.",
                text("Always on").font(k.ty.ui).color(c.text_muted)),
        ]))
        .child(card(k, "Workspace", vec![
            setting_row(k, "Workspace root", "Tools are confined to this directory.",
                text(s.current().cwd.display().to_string()).font(k.ty.mono).color(c.text_muted).truncate()),
            setting_row(k, "Skills folder", "Add a SKILL.md here to teach the agent a new playbook.",
                text(".rivo/skills/").font(k.ty.mono).color(c.text_muted)),
        ]))
}

fn provider_panel(s: &Rivo, k: &Kit, layer: &mut Vec<El>) -> El {
    let c = &k.c;
    let mut presets: Vec<String> = vec!["Mock provider".into(), "Kilo Code — free".into(), "Custom endpoint".into()];
    let current = match s.settings_preset { Preset::Mock => "Mock provider", Preset::KiloFree => "Kilo Code — free", Preset::Custom => "Custom endpoint" };
    let preset = select_box(s, k, layer, "provider-select", &presets, current, |s, v| {
        s.choose_preset(match v { "Kilo Code — free" => Preset::KiloFree, "Custom endpoint" => Preset::Custom, _ => Preset::Mock });
    });
    presets.clear();

    let protocols: Vec<String> = Protocol::all().iter().map(|p| p.label().to_string()).collect();
    let protocol = select_box(s, k, layer, "protocol-select", &protocols, s.model_config.protocol.label(), |s, v| {
        let p = match v {
            "Anthropic Messages" => Protocol::Anthropic,
            "Google Gemini" => Protocol::Gemini,
            _ => Protocol::OpenAiCompatible,
        };
        s.set_protocol(p);
    });

    let filtered = rivo_agent::catalog::filter(&s.catalog_models, &s.catalog_query.text, s.catalog_category);
    let search = TextInput::new(&s.catalog_query)
        .placeholder("Search 324 providers and 10k models…")
        .label("Search models.dev catalog")
        .width(Some(340.0))
        .leading_icon(k.glyph.search)
        .on_edit(|s: &mut Rivo| &mut s.catalog_query)
        .build(k);
    let refresh = Button::new(if s.catalog_busy { "Loading…" } else { "Refresh" })
        .variant(ButtonVariant::Ghost).control_size(ControlSize::Small)
        .on_press(|s: &mut Rivo| s.load_catalog()).build(k);
    let categories = row().w_full().gap(3.0).wrap().child(
        Category::all().into_iter().fold(row().gap(3.0).wrap(), |r, cat| {
            let active = s.catalog_category == cat;
            let mut pill = row().padding(6.0).radius(k.radius.sm).center()
                .on_press(move |s: &mut Rivo| s.set_catalog_category(cat));
            if active { pill = pill.fill(c.layer_03); } else { pill = pill.hover(c.hover); }
            r.child(pill.child(text(cat.label()).font(k.ty.small)
                .color(if active { c.text_base } else { c.text_muted })))
        }));
    let mut catalog_rows = Vec::new();
    for (i, m) in filtered.iter().take(40).enumerate() {
        let cost = if m.free { "free".to_string() } else { format!("${:.2}/${:.2} per 1M", m.input_cost, m.output_cost) };
        let meta = format!("{}  ·  {}  ·  {} ctx  ·  {}", m.provider_name, m.protocol.label(), format_tokens(m.context), cost);
        catalog_rows.push(row().w_full().min_h(52.0).padding(10.0).gap(12.0).center_y().justify_between()
            .child(col().grow(1.0).min_w(0.0).gap(3.0)
                .child(text(m.name.clone()).font(k.ty.ui_compact).color(c.text_base).truncate())
                .child(text(meta).font(k.ty.small).color(c.text_muted).truncate())
                .child(text(if m.description.is_empty() { m.id.clone() } else { m.description.clone() })
                    .font(k.ty.small).color(c.text_faint).truncate()))
            .child(Button::new("Use").variant(ButtonVariant::Contrast).control_size(ControlSize::Small)
                .on_press(move |s: &mut Rivo| s.use_catalog_model(i)).build(k)));
    }
    if catalog_rows.is_empty() {
        catalog_rows.push(row().w_full().padding(10.0).child(text(
            if s.catalog_busy { "Loading catalog…" } else { "No models match this filter." }
        ).font(k.ty.small).color(c.text_muted)));
    }
    let catalog_status = if s.catalog_status.is_empty() {
        "models.dev — free, open-source catalog; no API key needed.".to_string()
    } else {
        format!("{}  ·  showing {} of {}", s.catalog_status, filtered.len().min(40), filtered.len())
    };

    let kilo_models: Vec<String> = KILO_FREE_MODELS.iter().map(|m| m.id.to_string()).collect();
    let kilo_model = select_box(s, k, layer, "kilo-model-select", &kilo_models, &s.config.model, |s, v| {
        if let Some(i) = KILO_FREE_MODELS.iter().position(|m| m.id == v) {
            s.choose_free_model(i);
        }
    });

    col().w_full().gap(18.0)
        .child(text("Providers").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(card(k, "Connection", vec![
            setting_row(k, "Provider", "Mock runs offline; Kilo's free gateway needs no API key.", preset),
            setting_row(k, "Base URL", "Endpoint used for this provider.",
                rune_kit::TextInput::new(&s.base_url).label("Base URL").width(Some(260.0)).on_edit(|s: &mut Rivo| &mut s.base_url).build(k)),
            setting_row(k, "API key", "Read from the provider's environment variable when one is available.",
                rune_kit::TextInput::new(&s.api_key).label("API key").width(Some(260.0)).on_edit(|s: &mut Rivo| &mut s.api_key).build(k)),
            setting_row(k, "Model", "Model id sent with each request.",
                rune_kit::TextInput::new(&s.model_name).label("Model").width(Some(260.0)).on_edit(|s: &mut Rivo| &mut s.model_name).build(k)),
            setting_row(k, "Wire protocol", "OpenAI, Anthropic Messages or Google Gemini.", protocol),
            setting_row(k, "Test connection", "Checks the /models endpoint from a worker thread.",
                Button::new("Test").variant(ButtonVariant::Ghost).control_size(ControlSize::Regular)
                    .on_press(|s: &mut Rivo| s.test_connection()).build(k)),
        ]))
        .child(col().w_full().gap(10.0)
            .child(row().w_full().gap(8.0).center_y().child(search).child(refresh))
            .child(categories)
            .child(text(catalog_status).font(k.ty.small).color(c.text_muted).wrap())
            .child(card(k, "models.dev catalog — click Use to configure", catalog_rows)))
        .child(card(k, "Kilo Gateway (kept)", vec![
            setting_row(k, "Free model", "200 requests/hour per IP, no API key.", kilo_model),
        ]))
        .child(card(k, "Model & context", vec![
            setting_row(k, "Temperature", "Lower is more deterministic (0 – 2).",
                rune_kit::TextInput::new(&s.temperature_edit).label("Temperature").width(Some(120.0)).on_edit(|s: &mut Rivo| &mut s.temperature_edit).build(k)),
            setting_row(k, "Max output", "Maximum tokens generated per reply.",
                rune_kit::TextInput::new(&s.max_output_edit).label("Max output").width(Some(120.0)).on_edit(|s: &mut Rivo| &mut s.max_output_edit).build(k)),
            setting_row(k, "Context window", "Tokens kept before compaction.",
                rune_kit::TextInput::new(&s.context_window_edit).label("Context window").width(Some(140.0)).on_edit(|s: &mut Rivo| &mut s.context_window_edit).build(k)),
            setting_row(k, "Auto-compact", "Fold old turns into a short marker.",
                toggle_switch(k, s.agent_settings.auto_compact, |s| s.agent_settings.auto_compact = !s.agent_settings.auto_compact)),
            setting_row(k, "Recent messages", "How many turns stay untouched when compacting.",
                text(s.agent_settings.keep_recent.to_string()).font(k.ty.ui).color(c.text_muted)),
            setting_row(k, "Current usage", "Estimated from the active session.",
                text(format!("~{} / {} tokens", format_tokens(s.context_used), format_tokens(s.context_window))).font(k.ty.ui).color(c.text_muted)),
            setting_row(k, "System prompt", "Extra instructions appended to every request.",
                rune_kit::TextInput::new(&s.system_prompt).label("System prompt").width(Some(300.0)).on_edit(|s: &mut Rivo| &mut s.system_prompt).build(k)),
        ]))
        .child(
            col().w_full().gap(5.0).padding(12.0).radius(k.radius.md)
                .fill(c.warning.bg).border(1.0, c.warning.border)
                .child(text("Free tier notice").font(k.ty.ui_medium).color(c.warning.fg))
                .child(text("Free and open-weight models may log prompts for training — don't send confidential code. Check each provider's terms before use.")
                    .font(k.ty.small).color(c.warning.fg).wrap()),
        )
}

fn install_hint(spec: &InstallSpec) -> String {
    match spec {
        InstallSpec::Command { program, args, env } => {
            let mut line = format!("{} {}", program, args.join(" "));
            if !env.is_empty() {
                line.push_str(&format!("   env: {}", env.join(", ")));
            }
            line
        }
        InstallSpec::Remote { url } if !url.is_empty() => format!("remote  {url}"),
        InstallSpec::Skill { source, skill_id } => format!("{source}/{skill_id}"),
        _ => "unsupported".into(),
    }
}

fn marketplace_panel(s: &Rivo, k: &Kit) -> El {
    let c = &k.c;
    let mcp = s.settings_tab == SettingsTab::McpMarketplace;
    let title = if mcp { "MCP Marketplace" } else { "Skills Marketplace" };
    let source = if mcp { "Official MCP Registry" } else { "skills.sh" };
    let search = TextInput::new(&s.marketplace_search)
        .placeholder(if mcp { "Search MCP servers…" } else { "Search skills…" })
        .label("Search marketplace")
        .width(Some(420.0))
        .leading_icon(k.glyph.search)
        .on_edit(|s: &mut Rivo| &mut s.marketplace_search)
        .on_submit(|s: &mut Rivo, _| s.search_registry())
        .build(k);
    let button = Button::new(if s.registry_busy { "Searching…" } else { "Search" })
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press(|s: &mut Rivo| s.search_registry())
        .build(k);

    let mut rows = Vec::new();
    for (i, item) in s.registry_items.iter().enumerate() {
        let installed = s.installed_ids.contains(&item.id);
        let desc = if item.description.is_empty() { install_hint(&item.install) } else { item.description.clone() };
        let detail = format!("{}  ·  {}", item.publisher, install_hint(&item.install));
        let mut leading = col().grow(1.0).min_w(0.0).gap(3.0)
            .child(row().gap(6.0).center_y()
                .child(text(item.name.clone()).font(k.ty.ui).color(c.text_base).truncate())
                .child(icon(&BADGE, 12.0).color(if item.verified { c.success.fg } else { c.icon_faint })))
            .child(text(desc).font(k.ty.small).color(c.text_muted).wrap())
            .child(text(detail).font(k.ty.small).color(c.text_faint).truncate());
        if installed {
            leading = leading.child(text("Installed").font(k.ty.small).color(c.success.fg));
        }
        rows.push(row().w_full().min_h(56.0).padding(12.0).gap(14.0).center_y().justify_between()
            .child(leading)
            .child(Button::new(if installed { "Added" } else { "Add" })
                .variant(ButtonVariant::Contrast)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut Rivo| s.install_registry_item(i))
                .build(k)));
    }

    let status = if s.registry_status.is_empty() {
        format!("Free public source: {source}. No API key required.")
    } else {
        s.registry_status.clone()
    };
    col().w_full().gap(18.0)
        .child(text(title).font(rune::tok::type_::TITLE).color(c.text_base))
        .child(text(if mcp {
            "Browse the official MCP Registry. Publishers verify a GitHub or DNS namespace, but the registry does not scan server code — review a server before adding it. Added servers are written to ~/.config/rivo/mcp.json and load on restart."
        } else {
            "Browse public skills indexed by skills.sh. Install counts and partner audit results are shown on skills.sh; read a skill's SKILL.md before using it. One click downloads it to .rivo/skills in your workspace."
        }).font(k.ty.body).color(c.text_muted).wrap())
        .child(row().w_full().gap(8.0).center_y().child(search).child(button))
        .child(text(status).font(k.ty.small).color(c.text_muted).wrap())
        .child(card(k, if mcp { "Servers" } else { "Skills" }, rows))
}

fn skills_panel(s: &Rivo, k: &Kit) -> El {
    let c = &k.c;
    let mut rows = Vec::new();
    for skill in &s.skills {
        let name = skill.name.clone();
        let on = s.agent_settings.skill_enabled(&skill.name);
        rows.push(setting_row(k, &skill.name, &format!("{} — {}", skill.description, skill.source),
            toggle_switch(k, on, move |s: &mut Rivo| s.toggle_skill(&name))));
    }
    col().w_full().gap(18.0)
        .child(text("Skills").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(text("Short playbooks the agent loads on demand. Drop a SKILL.md into .rivo/skills/ or ~/.config/rivo/skills/.")
            .font(k.ty.body).color(c.text_muted).wrap())
        .child(card(k, "Available skills", rows))
}

fn tools_panel(s: &Rivo, k: &Kit) -> El {
    let c = &k.c;
    let mut rows = Vec::new();
    for tool in rivo_agent::tools::all_tools() {
        let name = tool.name().to_string();
        let on = s.agent_settings.tool_enabled(tool.name());
        let desc = tool.description().to_string();
        rows.push(setting_row(k, tool.name(), &desc,
            toggle_switch(k, on, move |s: &mut Rivo| s.toggle_tool(&name))));
    }
    col().w_full().gap(18.0)
        .child(text("Tools").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(text(format!("{} tools available. Disabled tools are removed from the model's tool list.", rows.len()))
            .font(k.ty.body).color(c.text_muted).wrap())
        .child(card(k, "Tool access", rows))
}

fn permissions_panel(s: &Rivo, k: &Kit, layer: &mut Vec<El>) -> El {
    let c = &k.c;
    let modes: Vec<String> = vec![PermissionMode::Auto.label().to_string(), PermissionMode::ReadOnly.label().to_string()];
    let mode = select_box(s, k, layer, "mode-select", &modes, s.agent_settings.permission.label(), |s, v| {
        s.set_permission(if v.starts_with("Read") { PermissionMode::ReadOnly } else { PermissionMode::Auto });
    });
    col().w_full().gap(18.0)
        .child(text("Permissions").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(card(k, "Safety", vec![
            setting_row(k, "Tool permission", "Auto allows tools; Read-only blocks writes and commands.", mode),
            setting_row(k, "Max steps", "Tool loop iterations allowed per turn.",
                text(s.agent_settings.max_steps.to_string()).font(k.ty.ui).color(c.text_muted)),
            setting_row(k, "Workspace confinement", "Absolute paths, .. and escaping symlinks are rejected.",
                text("Enforced").font(k.ty.ui).color(c.success.fg)),
            setting_row(k, "Command allowlist", "No shell metacharacters; 30 second timeout.",
                text("Enforced").font(k.ty.ui).color(c.success.fg)),
        ]))
        .child(card(k, "Advanced", vec![
            setting_row(k, "Endpoint", "Where chat requests are sent.",
                text(s.config.base_url.clone()).font(k.ty.mono).color(c.text_muted).truncate()),
            setting_row(k, "Restore defaults", "Reset model, context, tools and skills to built-in values.",
                Button::new("Restore").variant(ButtonVariant::Ghost).control_size(ControlSize::Regular)
                    .on_press(|s: &mut Rivo| s.restore_defaults()).build(k)),
        ]))
}

pub fn settings_page(s: &Rivo, k: &Kit, inset: f32) -> El {
    let c = &k.c;
    let mut layer = Vec::new();
    let panel = match s.settings_tab {
        SettingsTab::Provider => general_panel(s, k, &mut layer),
        SettingsTab::Connection => provider_panel(s, k, &mut layer),
        SettingsTab::McpMarketplace | SettingsTab::SkillsMarketplace => marketplace_panel(s, k),
        SettingsTab::Skills => skills_panel(s, k),
        SettingsTab::Tools => tools_panel(s, k),
        SettingsTab::Permissions => permissions_panel(s, k, &mut layer),
    };

    let status_tone = if s.connection_status.starts_with("Connection failed") { c.danger.fg } else { c.text_muted };
    let content = col().key("settings-content").w_full().min_h(0.0)
        .padding(26.0).gap(20.0).scroll(rune::ScrollSpec { stick_bottom: false })
        .child(panel.grow(1.0).min_w(0.0));
    let footer = row().key("settings-footer").w_full().min_h(52.0).padding(14.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], c.border_weak).gap(10.0).center_y()
        .child(Button::new("Apply & save").variant(ButtonVariant::Contrast)
            .control_size(ControlSize::Regular).on_press(|s: &mut Rivo| s.apply_settings()).build(k).key("settings-apply"))
        .child(Button::new(if s.testing { "Testing…" } else { "Test connection" })
            .variant(ButtonVariant::Ghost).control_size(ControlSize::Regular)
            .on_press(|s: &mut Rivo| s.test_connection()).build(k).key("settings-test"))
        .child(text(s.connection_status.clone()).font(k.ty.small).color(status_tone).wrap());
    let back = IconButton::new(&BACK).variant(IconButtonVariant::GhostMuted).help("Back to sessions")
        .on_press(|s: &mut Rivo| s.close_settings()).build(k).key("settings-back");

    // Full-page settings: no window-in-a-window, no frame inset, no toolbar. The nav and the
    // footer run edge to edge; only the content itself keeps its inner padding.
    let topbar = row().key("settings-topbar").w_full().min_h(46.0).pt(inset).padding(10.0)
        .gap(10.0).center_y().border_sides([0.0, 0.0, 1.0, 0.0], c.border_weak)
        .child(back)
        .child(text("Settings").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(row().grow(1.0))
        .child(Button::new("Restore defaults").variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Regular).on_press(|s: &mut Rivo| s.restore_defaults()).build(k));

    let body = col().key("settings-body").w_full().h_full().min_h(0.0)
        .child(topbar)
        .child(row().key("settings-body-row").w_full().grow(1.0).min_h(0.0)
            .child(settings_nav(s, k).h_full())
            .child(col().key("settings-right").grow(1.0).h_full().min_h(0.0)
                .child(content.grow(1.0))
                .child(footer)));

    rune_kit::root(body, layer)
}
