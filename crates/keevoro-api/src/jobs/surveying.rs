//! `places.survey`: finding which folders in a drive or folder hold photos
//! and videos, by reading folder listings only. No file is opened and
//! nothing is remembered: the answer is a list of suggestions for the
//! person to choose from.

use super::{spawn, Context, HintView, JobHandle, JobKind, Listener, Said, SurveyEvent, SurveyView};
use crate::backups::Named;
use crate::error::Result;
use keevoro_catalog::Catalog;
use keevoro_engine::friendly_path;
use std::path::Path;
use std::time::Instant;

pub(super) fn start(
    ctx: &Context,
    place: &Path,
    backup: Option<&Named>,
    listener: Option<Listener>,
) -> Result<JobHandle> {
    // What is a source already, as it is shown, to say so beside each
    // suggestion.
    let sources: Vec<String> = backup
        .and_then(|backup| Catalog::open_in_library(&backup.home).and_then(|c| c.sources()).ok())
        .map(|list| list.iter().map(|s| friendly_path(&s.root)).collect())
        .unwrap_or_default();
    let job = ctx.jobs.begin(JobKind::Survey, backup, listener)?;
    let handle = job.handle.clone();
    let path = place.display().to_string();
    ctx.log.say(&format!("Finding where the photos are in {path}"));
    job.note_line(&format!("Finding where the photos are in {path}"));
    let root = place.to_path_buf();
    spawn(ctx, &job, move |ctx, job, ending| {
        let started = Instant::now();
        let stop = job.stop_switch();
        let found = {
            let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::Surveying);
            keevoro_scan::survey::survey(&root, &keevoro_core::SkipRules::default(), Some(&stop), &mut |p| {
                ctx.jobs.heard(
                    job,
                    Said::Survey(SurveyEvent {
                        folders: p.folders_seen,
                        files: p.files_seen,
                        photos: p.photos,
                        videos: p.videos,
                    }),
                    true,
                );
            })
        };
        let seconds = started.elapsed().as_secs_f32();
        ctx.log.say(&format!(
            "Looked at {} files in {} folders in {seconds:.1} seconds without opening any: {} folders suggested{}",
            found.files_seen,
            found.folders_seen,
            found.hints.len(),
            if found.stopped { " (stopped early)" } else { "" }
        ));
        let view = SurveyView {
            hints: found
                .hints
                .into_iter()
                .map(|h| {
                    let folder = friendly_path(&h.folder);
                    HintView {
                        already_a_source: sources.contains(&folder),
                        folder,
                        photos: h.photos,
                        videos: h.videos,
                        bytes: h.bytes,
                        likely_photos: h.likely == keevoro_scan::survey::Likely::Photos,
                    }
                })
                .collect(),
            folders: found.folders_seen,
            files: found.files_seen,
            unreadable: found.unreadable,
            seconds,
            stopped: found.stopped,
        };
        let last = Said::Surveyed(view);
        ctx.jobs.heard(job, last.clone(), true);
        ending.with(Some(last));
    })?;
    Ok(handle)
}
