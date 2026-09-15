use xchendev_tools_lib::app_state::AppState;

#[test]
fn confirmation_token_is_single_use() {
    let state = AppState::new();
    let (token, _) = state
        .issue_confirmation("p1", "npm run dev", "C:\\proj", "frontend")
        .expect("issue");
    state
        .consume_confirmation(&token, "p1", "npm run dev", "C:\\proj", "frontend")
        .expect("consume");
    let again = state.consume_confirmation(&token, "p1", "npm run dev", "C:\\proj", "frontend");
    assert!(again.is_err());
}

#[test]
fn confirmation_invalid_when_command_changes() {
    let state = AppState::new();
    let (token, _) = state
        .issue_confirmation("p1", "npm run dev", "C:\\proj", "frontend")
        .expect("issue");
    let result = state.consume_confirmation(&token, "p1", "npm run build", "C:\\proj", "frontend");
    assert!(result.is_err());
}
