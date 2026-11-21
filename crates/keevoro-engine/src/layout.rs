//! How a backup is laid out where it is sent, explained for the person who
//! opens the folder one day and wonders what they are looking at.
//!
//! The explanation is written into the backup itself, as a plain text file
//! beside the folders it describes, so it is there even if Keevoro is not.
//! The app shows the same parts on the screen that backs up
//! (`ui/src/App.tsx`); when a folder is added here, add it there too. A test
//! in `send.rs` fails if a backup ever holds a folder this file leaves out.

/// The guide's name in the backup. Plain words, so it explains itself in a
/// file listing and sorts near the folders it describes.
pub const GUIDE_NAME: &str = keevoro_core::BACKUP_GUIDE;

/// One part of a backup: the folder's name, and what a person needs to know.
pub struct Part {
    pub folder: &'static str,
    pub what: &'static str,
}

/// Every folder a backup can hold, most important first.
pub const PARTS: [Part; 3] = [
    Part {
        folder: "originals",
        what: "Your photos and videos, exactly as they were on your sources. Not resized, not \
               renamed beyond what a folder needs, not changed in any way: each one was checked \
               against the original after it was copied. This folder is the backup. Inside, files \
               are grouped the way Keevoro showed them to you: one folder per collection, and \
               \"By date\" (by year, then month) for anything that did not belong to one.",
    },
    Part {
        folder: "renditions",
        what: "Smaller copies that Keevoro made so a gallery can show your photos quickly. A \
               rendition is a version of a photo or video made for looking at, not for keeping. \
               \"thumb\" holds small pictures for a grid, \"display\" holds screen-sized ones, \
               \"poster\" holds a still picture for each video, and \"video\" holds versions of old \
               videos that modern devices can play. Each is named after a fingerprint of the \
               original, not its file name. Everything here can be made again from the originals, \
               so it is safe to delete if you need the room.",
    },
    Part {
        folder: "index",
        what: "The list of what is in the backup: every photo and video, its date, which \
               collection it belongs to and where its renditions are. gallery.json is that list \
               in a form a gallery website or app can read. It can also be made again from the \
               originals.",
    },
];

/// What the guide adds when the backup holds other files: where they are.
/// Said only then, so a backup of photos and videos alone reads exactly as
/// it always did.
pub const OTHER_FILES_PART: &str = "Other files you asked Keevoro to carry along, such as music and \
     documents, are in \"Other files\" inside \"originals\", in the folders they came from, starting \
     with the name of the folder or drive they were on. They are exact copies too, but Keevoro made \
     no renditions of them, and the index lists them in albums named after their folders.";

/// The guide as plain text. Lines end the way Windows Notepad expects,
/// which every other editor also reads. `with_other_files` when the backup
/// holds other files, which the guide then says where to find.
pub fn guide(with_other_files: bool) -> String {
    let mut text = String::from(
        "What is in this backup\r\n\
         ======================\r\n\r\n\
         Keevoro made this backup. This note explains the folders beside it.\r\n\r\n\
         The short version: your photos and videos are in \"originals\", untouched.\r\n\
         Everything else is there to make them quick to look through, and can be\r\n\
         made again.\r\n",
    );
    for part in &PARTS {
        text.push_str("\r\n");
        text.push_str(part.folder);
        text.push_str("\r\n");
        text.push_str(&"-".repeat(part.folder.len()));
        text.push_str("\r\n");
        text.push_str(&wrapped(part.what, 76));
        if with_other_files && part.folder == keevoro_core::layout::ORIGINALS {
            text.push_str("\r\n");
            text.push_str(&wrapped(OTHER_FILES_PART, 76));
        }
    }
    text.push_str(
        "\r\nGetting your photos back\r\n\
         ------------------------\r\n\
         You do not need Keevoro. Copy the \"originals\" folder to wherever you want\r\n\
         your photos to be. They are ordinary files.\r\n\r\n\
         Adding to this backup\r\n\
         ---------------------\r\n\
         Back up to this same place again and Keevoro adds only what is new. It never\r\n\
         replaces or removes a file in \"originals\".\r\n",
    );
    text
}

/// Breaks `text` into lines of at most `width` characters, at spaces.
fn wrapped(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = 0;
    for word in text.split_whitespace() {
        if line > 0 && line + 1 + word.len() > width {
            out.push_str("\r\n");
            line = 0;
        }
        if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += word.len();
    }
    out.push_str("\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_explains_every_part_and_what_a_rendition_is() {
        let guide = guide(false);
        for part in &PARTS {
            assert!(
                guide.contains(&format!("\r\n{}\r\n", part.folder)),
                "{} has a heading",
                part.folder
            );
        }
        assert!(guide.contains("A rendition is a version of a photo or video made for looking at"));
        assert!(guide.contains("You do not need Keevoro"));
    }

    #[test]
    fn the_guide_says_where_other_files_are_only_when_there_are_some() {
        assert!(!guide(false).contains("Other files"));
        let with = guide(true);
        assert!(with.contains("\"Other files\" inside \"originals\""));
        assert!(with.lines().all(|l| l.len() <= 80));
        assert!(!with.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn the_guide_reads_in_notepad_and_fits_a_narrow_window() {
        let guide = guide(false);
        assert!(
            !guide.replace("\r\n", "").contains('\n'),
            "every line ends the Windows way"
        );
        assert!(
            guide.lines().all(|l| l.len() <= 80),
            "no line is wider than 80 characters"
        );
    }
}
