use super::DISCLAIMER_CONTENT;
use std::path::PathBuf;

/// Helper function to display the legal disclaimer if needed
pub fn handle_disclaimer(accept_disclaimer: bool, approved_disclaimer_file_path: &PathBuf) {
    if !accept_disclaimer && !approved_disclaimer_file_path.exists() {
        // Include the content of the disclaimer file at compile time
        let accepted_disclaimer = dialoguer::Confirm::new()
            .with_prompt(DISCLAIMER_CONTENT)
            .default(false)
            .report(false)
            .interact()
            .expect("IO Error: Could not query if the disclaimer was approved or not ");

        if !accepted_disclaimer {
            panic!("You have to approve the legal disclaimer to continue. Exiting.");
        }
    }
    // Create the file to avoid showing the disclaimer again
    if !approved_disclaimer_file_path.exists() {
        std::fs::File::create(approved_disclaimer_file_path)
            .expect("Failed to create the approved disclaimer file");
    }
}
