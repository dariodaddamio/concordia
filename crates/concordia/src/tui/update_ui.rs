use crate::update::{JobSnapshot, PreviewLine as P, UpdatePreview, UpdateStage as S};
use std::time::Duration;

fn page(lines: Vec<P>) -> UpdatePreview {
    UpdatePreview {
        lines,
        history: Vec::new(),
        history_expanded: false,
        install_available: false,
        release_url: None,
    }
}

pub(super) fn running(snapshot: &JobSnapshot, elapsed: Duration, expanded: bool) -> UpdatePreview {
    let current = match snapshot.stage {
        None | Some(S::Preparing) => 0,
        Some(S::Building | S::Downloading) => 1,
        Some(S::Verifying) => 2,
        Some(S::Installing) => 3,
        Some(S::Finishing) => 4,
    };
    let download = snapshot.download;
    let steps = [
        "Prepare",
        if download { "Download" } else { "Build" },
        "Verify",
        "Install",
        "Restart",
    ];
    let mut lines = vec![
        P::Title("Updating Concordia".into()),
        P::Field("Elapsed", crate::delegate::fmt_elapsed(elapsed)),
    ];
    if expanded {
        lines.push(P::Field("Now", snapshot.status.clone()));
        lines.push(P::Field("Warnings", snapshot.warning_count.to_string()));
        lines.push(P::Field("Errors", snapshot.error_count.to_string()));
        lines.push(P::Blank);
        diagnostics(&mut lines, snapshot);
        lines.push(P::Hint("D update progress \u{b7} Esc cancel update".into()));
        return page(lines);
    }
    lines.push(P::Blank);
    for (index, step) in steps.into_iter().enumerate() {
        let label = if index < current {
            "Done"
        } else if index == current {
            "Now"
        } else {
            "Next"
        };
        let value = if index == current && !snapshot.status.is_empty() {
            format!(
                "{step}  \u{b7}  {}",
                snapshot.status.lines().next().unwrap_or_default()
            )
        } else {
            step.into()
        };
        lines.push(P::Field(label, value));
    }
    if !snapshot.detail.is_empty() {
        lines.push(P::Text(snapshot.detail.clone()));
    }
    lines.extend([
        P::Blank,
        P::Field("Warnings", snapshot.warning_count.to_string()),
        P::Field("Errors", snapshot.error_count.to_string()),
        P::Blank,
    ]);
    lines.push(P::Hint("D build details \u{b7} Esc cancel update".into()));
    page(lines)
}

pub(super) fn failed(snapshot: &JobSnapshot, error: &str) -> UpdatePreview {
    let mut lines = vec![
        P::Title("Update could not finish".into()),
        P::Text("Concordia is still running. Review the details, then retry /update.".into()),
        P::Blank,
    ];
    if !snapshot.status.is_empty() {
        lines.push(P::Field("Stage", snapshot.status.clone()));
    }
    lines.push(P::Text(
        super::text::terminal_text(&crate::redact::redact(error))
            .0
            .chars()
            .take(4096)
            .collect(),
    ));
    lines.push(P::Blank);
    diagnostics(&mut lines, snapshot);
    page(lines)
}

fn diagnostics(lines: &mut Vec<P>, snapshot: &JobSnapshot) {
    lines.push(P::Kind("Build details".into()));
    if snapshot.diagnostics.is_empty() {
        lines.push(P::Text("No compiler diagnostics reported.".into()));
    }
    for diagnostic in &snapshot.diagnostics {
        let suffix = diagnostic
            .code
            .as_ref()
            .map(|code| format!(" [{code}]"))
            .unwrap_or_default();
        lines.push(P::Kind(format!("{}{}", diagnostic.severity, suffix)));
        if !diagnostic.detail.is_empty() {
            for line in crate::text_safe::terminal_safe(&diagnostic.detail).lines() {
                lines.push(P::Text(line.to_owned()));
            }
        } else {
            lines.push(P::Text(diagnostic.message.clone()));
            if let Some(source) = &diagnostic.source {
                lines.push(P::Text(source.clone()));
            }
        }
        lines.push(P::Blank);
    }
    let count = snapshot.warning_count.saturating_add(snapshot.error_count);
    if count > snapshot.diagnostics.len() {
        lines.push(P::Text(format!(
            "Showing {} of {count} diagnostics.",
            snapshot.diagnostics.len()
        )));
    }
}
