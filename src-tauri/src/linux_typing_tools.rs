/// Filter installed tools by known session restrictions, without claiming that
/// an unknown Wayland compositor supports wtype's virtual-keyboard protocol.
pub(crate) fn available_tools(
    wayland: bool,
    kde_wayland: bool,
    gnome_wayland: bool,
    is_installed: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut tools = vec!["auto".to_string()];
    for (tool, session_allowed) in [
        ("wtype", wayland && !kde_wayland && !gnome_wayland),
        ("kwtype", kde_wayland),
        ("dotool", true),
        ("ydotool", true),
        // XWayland only reaches X11 clients; it is not a general Wayland option.
        ("xdotool", !wayland),
    ] {
        if session_allowed && is_installed(tool) {
            tools.push(tool.to_string());
        }
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::available_tools;

    const SESSIONS: [(bool, bool, bool); 4] = [
        (true, true, false),   // KDE Wayland
        (true, false, true),   // GNOME Wayland
        (true, false, false),  // Other/unknown Wayland compositor
        (false, false, false), // X11
    ];

    #[test]
    fn kde_wayland_excludes_wtype_and_xdotool() {
        assert_eq!(
            available_tools(true, true, false, |_| true),
            ["auto", "kwtype", "dotool", "ydotool"]
        );
    }

    #[test]
    fn gnome_wayland_excludes_wtype_kwtype_and_xdotool() {
        assert_eq!(
            available_tools(true, false, true, |_| true),
            ["auto", "dotool", "ydotool"]
        );
    }

    #[test]
    fn other_wayland_retains_wtype_without_a_capability_probe() {
        assert_eq!(
            available_tools(true, false, false, |_| true),
            ["auto", "wtype", "dotool", "ydotool"]
        );
    }

    #[test]
    fn x11_excludes_wayland_only_tools() {
        assert_eq!(
            available_tools(false, false, false, |_| true),
            ["auto", "dotool", "ydotool", "xdotool"]
        );
    }

    #[test]
    fn no_installed_tools_still_offers_auto_in_every_session() {
        for (wayland, kde, gnome) in SESSIONS {
            assert_eq!(available_tools(wayland, kde, gnome, |_| false), ["auto"]);
        }
    }

    #[test]
    fn each_tool_must_be_installed_to_be_offered() {
        for (wayland, kde, gnome) in SESSIONS {
            let all = available_tools(wayland, kde, gnome, |_| true);
            for tool in ["wtype", "kwtype", "dotool", "ydotool", "xdotool"] {
                let only_installed = available_tools(wayland, kde, gnome, |name| name == tool);
                let expected = if all.iter().any(|name| name == tool) {
                    vec!["auto", tool]
                } else {
                    vec!["auto"]
                };
                assert_eq!(
                    only_installed, expected,
                    "session {wayland}/{kde}/{gnome}, {tool}"
                );
            }
        }
    }
}
