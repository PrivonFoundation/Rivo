//! Rivo's desktop views. Layout follows a friendly chat-app shape: a narrow icon rail,
//! a project-grouped session sidebar, a tab strip, a floating composer and a right inspector.

use std::fmt::Write;

use rivo_agent::{
    KILO_FREE_MODELS, Message, PermissionMode, Preset, Protocol, Role, ToolCall,
};
use rune::{Color, El, Icon, col, el, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown, TextArea,
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
static CLOSE: Icon = Icon {
    name: "rivo-close", view: 16.0,
    body: r#"<path d="M5 5L11 11M11 5L5 11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#,
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
            row().size(28.0, 28.0).radius(14.0).center().fill(rune::oklch(0.62, 0.12, 22.0))
                .child(text("R").font(k.ty.small_medium).color(Color::WHITE))
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
        let (name, hue) = PROJECTS.iter().find(|(n, _)| *n == project).copied().unwrap_or((project, 250.0));
        list = list.child(
            row().key(format!("project-{name}")).w_full().gap(7.0).center_y()
                .on_press({
                    let name = name.to_string();
                    move |s: &mut Rivo| s.toggle_project(&name)
                })
                .child(icon(&CHEVRON, 12.0).color(c.icon_faint).rotate(if closed { 0.0 } else { std::f32::consts::PI / 2.0 }))
                .child(row().size(14.0, 14.0).radius(4.0).center().fill(rune::oklch(0.64, 0.12, hue))
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

fn message(m: &Message, k: &Kit, streaming: bool, i: usize) -> El {
    let c = &k.c;
    let user = m.role == Role::User;
    let mut card = col().key(format!("msg-{i}")).w_full().max_w(760.0)
        .radius(k.radius.lg).border(1.0, if user { c.border_weak } else { c.border_muted })
        .fill(if user { c.layer_02 } else { c.layer_01 }).padding(14.0).gap(10.0);
    if user {
        card = card.child(text("You").font(k.ty.small_medium).color(c.text_faint));
    } else {
        card = card.child(row().gap(7.0).center_y()
            .child(row().size(16.0, 16.0).radius(4.0).center().fill(rune_theme_opencode::brand(k.dark))
                .child(text("R").font(k.ty.small_medium).color(Color::WHITE)))
            .child(text("Rivo").font(k.ty.small_medium).color(c.text_faint)));
    }
    if !m.text.is_empty() {
        let body = if user {
            text(m.text.clone()).font(k.ty.body).color(c.text_base).wrap().selectable(true)
        } else {
            el().key(format!("md-{i}")).child(Markdown::new(m.text.clone())
                .streaming(streaming).reveal(if streaming { 0.12 } else { 0.0 }).selectable(true).build(k))
        };
        card = card.child(body);
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
        list = list.child(message(m, k, streaming, i));
    }
    if let Some(removed) = s.compaction_notice {
        list = list.child(text(format!("Context compacted — {removed} older messages folded."))
            .font(k.ty.small).color(k.c.text_faint));
    }
    list
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
        .child(row().size(44.0, 44.0).radius(12.0).center().fill(rune_theme_opencode::brand(k.dark))
            .child(text("R").font(rune::tok::type_::TITLE).color(Color::WHITE)))
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

pub fn composer(s: &Rivo, k: &Kit) -> El {
    let c = &k.c;
    let busy = s.busy();
    let send = if busy {
        IconButton::new(&STOP).variant(IconButtonVariant::GhostMuted).help("Stop")
            .on_press(|s: &mut Rivo| s.stop()).build(k)
    } else {
        IconButton::new(&SEND).variant(IconButtonVariant::Submit).help("Send message")
            .on_press(|s: &mut Rivo| s.submit()).build(k)
    };
    let usage = format!("~{:.1}k / {}k context", s.context_used as f32 / 1000.0, s.context_window / 1000);
    col().key("composer").w_full().max_w(820.0)
        .radius(k.radius.lg).border(1.0, c.border_base).fill(c.layer_01)
        .padding(10.0).gap(8.0)
        .child(TextArea::new(&s.draft)
            .placeholder("Type a message — / for commands, @ for skills or files…")
            .label("Message").width(None).rows(2).auto_grow(2, 6).max_height(150.0)
            .on_edit(|s: &mut Rivo| &mut s.draft)
            .on_submit(|s: &mut Rivo, _| s.submit()).build(k))
        .child(row().justify_between().center_y()
            .child(row().gap(8.0).center_y()
                .child(Button::new(format!("{}  ⌄", s.config.model))
                    .variant(ButtonVariant::Ghost).control_size(ControlSize::Small)
                    .on_press(|s: &mut Rivo| { s.settings_tab = SettingsTab::Model; s.open_settings(); }).build(k))
                .child(text(usage).font(k.ty.small).color(c.text_faint)))
            .child(row().gap(6.0).center_y()
                .child(IconButton::new(&PLUS).variant(IconButtonVariant::GhostMuted).help("Skills")
                    .on_press(|s: &mut Rivo| { s.settings_tab = SettingsTab::Skills; s.open_settings(); }).build(k))
                .child(send)))
}

// ---------------------------------------------------------------------------- inspector

fn tab_pill(s: &Rivo, k: &Kit, label: &str, tab: InspectorTab) -> El {
    let c = &k.c;
    let active = s.inspector_tab == tab;
    let mut b = row().padding(7.0).radius(k.radius.sm).center()
        .on_press(move |s: &mut Rivo| s.inspector_tab = tab);
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
        .child(tab_pill(s, k, "Details", InspectorTab::Details))
        .child(tab_pill(s, k, "Tools", InspectorTab::Tools))
        .child(tab_pill(s, k, "Skills", InspectorTab::Skills))
        .child(tab_pill(s, k, "Context", InspectorTab::Context)));

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
        InspectorTab::Context => {
            let pct = (s.context_used as f32 / s.context_window.max(1) as f32 * 100.0).min(100.0);
            body = body.child(text("Context window").font(k.ty.ui_medium).color(c.text_base))
                .child(field("Used", format!("~{} tokens ({pct:.0}%)", s.context_used)))
                .child(field("Window", format!("{} tokens", s.context_window)))
                .child(el().h(6.0).w_full().radius(3.0).fill(c.layer_03).child(
                    el().h(6.0).w(pct.max(1.0)).radius(3.0).fill(if pct > 85.0 { c.danger.fg } else { c.text_accent })));
            if s.agent_settings.auto_compact {
                body = body.child(text(format!("Auto-compact after {} recent messages are kept.", s.agent_settings.keep_recent))
                    .font(k.ty.small).color(c.text_muted).wrap());
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
        .child(settings_nav_item(s, k, "Model & context", SettingsTab::Model, &GRID))
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
    let mut presets: Vec<String> = vec!["Mock provider".into(), "Kilo Code — free".into(), "Custom endpoint".into()];
    let current_preset = match s.settings_preset {
        Preset::Mock => &presets[0],
        Preset::KiloFree => &presets[1],
        Preset::Custom => &presets[2],
    }.clone();
    let preset = select_box(s, k, layer, "preset-select", &presets, &current_preset, |s, v| {
        s.choose_preset(match v {
            "Kilo Code — free" => Preset::KiloFree,
            "Custom endpoint" => Preset::Custom,
            _ => Preset::Mock,
        });
    });
    presets.clear();

    let models: Vec<String> = KILO_FREE_MODELS.iter().map(|m| m.id.to_string()).collect();
    let model = select_box(s, k, layer, "model-select", &models, &s.config.model, |s, v| {
        if let Some(i) = KILO_FREE_MODELS.iter().position(|m| m.id == v) {
            s.choose_free_model(i);
        }
    });

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
            setting_row(k, "Provider", "How Rivo reaches the model.", preset),
            setting_row(k, "Model", "The model used for new turns and tool calls.", model),
            setting_row(k, "Permissions", "Read-only blocks writes and commands.", permission),
            setting_row(k, "Auto-compact context", "Fold old turns into a marker when the window fills up.",
                toggle_switch(k, s.agent_settings.auto_compact, |s| s.agent_settings.auto_compact = !s.agent_settings.auto_compact)),
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
    col().w_full().gap(18.0)
        .child(text("Provider").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(card(k, "Connection", vec![
            setting_row(k, "Provider", "Mock runs offline; Kilo's free gateway needs no API key.", preset),
            setting_row(k, "Base URL", "OpenAI-compatible endpoint or Kilo gateway.",
                rune_kit::TextInput::new(&s.base_url).width(Some(260.0)).on_edit(|s: &mut Rivo| &mut s.base_url).build(k)),
            setting_row(k, "API key", "Not needed for the free gateway or local models.",
                rune_kit::TextInput::new(&s.api_key).width(Some(260.0)).on_edit(|s: &mut Rivo| &mut s.api_key).build(k)),
            setting_row(k, "Model", "Model id sent with each request.",
                rune_kit::TextInput::new(&s.model_name).width(Some(260.0)).on_edit(|s: &mut Rivo| &mut s.model_name).build(k)),
            setting_row(k, "Test connection", "Checks the /models endpoint from a worker thread.",
                Button::new("Test").variant(ButtonVariant::Ghost).control_size(ControlSize::Regular)
                    .on_press(|s: &mut Rivo| s.test_connection()).build(k)),
        ]))
        .child(
            col().w_full().gap(5.0).padding(12.0).radius(k.radius.md)
                .fill(c.warning.bg).border(1.0, c.warning.border)
                .child(text("Free tier notice").font(k.ty.ui_medium).color(c.warning.fg))
                .child(text("200 requests/hour per IP. Auto Free and some providers may log prompts for training — don't send confidential code. NVIDIA endpoints are trial use only.")
                    .font(k.ty.small).color(c.warning.fg).wrap()),
        )
}

fn model_panel(s: &Rivo, k: &Kit, layer: &mut Vec<El>) -> El {
    let c = &k.c;
    let protocols: Vec<String> = Protocol::all().iter().map(|p| p.label().to_string()).collect();
    let protocol = select_box(s, k, layer, "protocol-select", &protocols, s.model_config.protocol.label(), |s, v| {
        let p = match v {
            "Anthropic Messages" => Protocol::Anthropic,
            "Google Gemini" => Protocol::Gemini,
            _ => Protocol::OpenAiCompatible,
        };
        s.set_protocol(p);
    });
    col().w_full().gap(18.0)
        .child(text("Model & context").font(rune::tok::type_::TITLE).color(c.text_base))
        .child(card(k, "Model", vec![
            setting_row(k, "Wire protocol", "OpenAI, Anthropic Messages or Google Gemini.", protocol),
            setting_row(k, "Suggested base URL", &s.model_config.base_hint,
                text(s.model_config.base_hint.clone()).font(k.ty.mono).color(c.text_muted).truncate()),
            setting_row(k, "Temperature", "Lower is more deterministic (0 – 2).",
                rune_kit::TextInput::new(&s.temperature_edit).width(Some(120.0)).on_edit(|s: &mut Rivo| &mut s.temperature_edit).build(k)),
            setting_row(k, "Max output", "Maximum tokens generated per reply.",
                rune_kit::TextInput::new(&s.max_output_edit).width(Some(120.0)).on_edit(|s: &mut Rivo| &mut s.max_output_edit).build(k)),
        ]))
        .child(card(k, "Context window", vec![
            setting_row(k, "Window size", "Tokens kept in the conversation before compaction.",
                rune_kit::TextInput::new(&s.context_window_edit).width(Some(140.0)).on_edit(|s: &mut Rivo| &mut s.context_window_edit).build(k)),
            setting_row(k, "Auto-compact", "Fold old turns into a short marker.",
                toggle_switch(k, s.agent_settings.auto_compact, |s| s.agent_settings.auto_compact = !s.agent_settings.auto_compact)),
            setting_row(k, "Recent messages", "How many turns stay untouched when compacting.",
                text(s.agent_settings.keep_recent.to_string()).font(k.ty.ui).color(c.text_muted)),
            setting_row(k, "Current usage", "Estimated from the active session.",
                text(format!("~{} / {} tokens", s.context_used, s.context_window)).font(k.ty.ui).color(c.text_muted)),
        ]))
        .child(card(k, "Instructions", vec![
            setting_row(k, "System prompt", "Extra instructions appended to every request.",
                rune_kit::TextInput::new(&s.system_prompt).width(Some(300.0)).on_edit(|s: &mut Rivo| &mut s.system_prompt).build(k)),
        ]))
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
        SettingsTab::Model => model_panel(s, k, &mut layer),
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
