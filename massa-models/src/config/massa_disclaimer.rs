use super::COMMUNITY_CHARTER_CONTENT;
use std::path::PathBuf;

/// Helper function to display the Community Charter if needed
pub fn handle_disclaimer(
    auto_accept_community_charter: bool,
    approved_community_charter_file_path: &PathBuf,
) {
    if !auto_accept_community_charter && !approved_community_charter_file_path.exists() {
        let mut prompt = COMMUNITY_CHARTER_CONTENT.to_string();
        prompt.push_str("\n\nDo you accept the Community Charter?");

        let accepted_community_charter = dialoguer::Confirm::new()
            .with_prompt(prompt.clone())
            .default(false)
            .report(false)
            .interact()
            .unwrap_or_else(|_| {
                prompt.push_str("\n\nError: Not in a terminal, could not get user input.\n\n");
                print!("{}", prompt);
                false
            });

        if !accepted_community_charter {
            panic!("You have to approve the Community Charter to continue. You can read it in 'COMMUNITY_CHARTER.md' and re-run with the flag '-a' or '--accept-community-charter' to automatically accept them. Exiting.");
        }
    }
    // Create the file to avoid showing the disclaimer again
    if !approved_community_charter_file_path.exists() {
        std::fs::File::create(approved_community_charter_file_path)
            .expect("Failed to create the approved disclaimer file");
    }
}