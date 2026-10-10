use super::*;

#[test]
fn any_ai_is_started_on_the_same_context_with_one_line_every_shell_accepts() {
    let prompt = resume_prompt("0123abcd", ".atelier/sessions/0123abcd.md");
    assert!(!prompt.contains(['"', '\n', '\'', '`', '$']), "{prompt}");
    assert!(prompt.contains(".atelier/sessions/0123abcd.md"));
    for (agent, start) in [
        (agents::Agent::Claude, "claude \""),
        (agents::Agent::Codex, "codex \""),
        (agents::Agent::Gemini, "gemini -i \""),
    ] {
        let command = resume_command(agent, &prompt);
        assert!(
            command.starts_with(start) && command.ends_with('"') && !command.contains('\n'),
            "{command}"
        );
        assert_eq!(
            agents::detect(&command),
            Some(agent),
            "the IDE would recognize it as that AI"
        );
    }
}
