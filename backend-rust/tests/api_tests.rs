use backend_rust::config::oauth::get_oauth_config;
use backend_rust::services::agent_prompts::get_prompt;

#[test]
fn test_agent_prompts() {
    let general = get_prompt(Some("general_reasoning"));
    assert!(general.system.contains("MutexFlow"));

    let extraction = get_prompt(Some("data_extraction"));
    assert!(extraction.system.contains("entities"));

    let drafting = get_prompt(Some("professional_drafting"));
    assert!(drafting.system.contains("writing assistant"));
}

#[test]
fn test_oauth_configs() {
    let providers = [
        "slack", "github", "google", "microsoft", "hubspot",
        "salesforce", "jira", "linear", "notion", "confluence",
    ];

    for provider in providers {
        let config = get_oauth_config(provider);
        assert!(config.is_some(), "Provider {} should have OAuth config", provider);
    }

    assert!(get_oauth_config("unknown_provider").is_none());
}
