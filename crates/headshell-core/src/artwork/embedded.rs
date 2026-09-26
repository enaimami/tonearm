//! A local file's cover: the picture in its tags, then an image in its
//! folder (D-076).
//!
//! What symphonia gives was checked in its source before this was written:
//! ID3v2 `APIC` (MP3), the FLAC `PICTURE` block, MP4 `covr` and Vorbis
//! `METADATA_BLOCK_PICTURE` (Ogg) all arrive as `Visual`s in a metadata
//! revision. An MP3's ID3v2 tag is read by the probe and is a revision of its
//! own, before the container's; that is why every revision is looked at, not
//! only the first. No reader applies `limit_visual_bytes`: a huge picture
//! arrives whole, and [`super::image`] is where it is refused.

use std::path::{Path, PathBuf};

use super::ArtworkSource;
use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};
use crate::provider::ArtworkImage;

/// The picture in the file's tags: the one marked as the front cover, or
/// the first one when none is marked.
///
/// `Ok(None)`: the file opened and has no picture.
///
/// # Errors
/// If the file cannot be opened or is not an audio file symphonia reads.
#[cfg(feature = "audio")]
pub(crate) fn embedded(path: &Path) -> Result<Option<ArtworkImage>> {
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardVisualKey, Visual};

    let file =
        std::fs::File::open(path).map_err(|source| io_err(Stage::ArtworkRead, path, source))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut reader = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            Default::default(),
            MetadataOptions::default(),
        )
        .map_err(|source| {
            Error::new(
                Stage::ArtworkRead,
                ErrorKind::Audio {
                    detail: format!("could not open {}: {source}", path.display()),
                },
            )
        })?;

    let mut front: Option<Visual> = None;
    let mut first: Option<Visual> = None;
    let mut log = reader.metadata();
    loop {
        if let Some(revision) = log.current() {
            let visuals = revision.media.visuals.iter().chain(
                revision
                    .per_track
                    .iter()
                    .flat_map(|track| &track.metadata.visuals),
            );
            for visual in visuals {
                if front.is_none() && visual.usage == Some(StandardVisualKey::FrontCover) {
                    front = Some(visual.clone());
                }
                if first.is_none() {
                    first = Some(visual.clone());
                }
            }
        }
        if front.is_some() || log.pop().is_none() {
            break;
        }
    }

    Ok(front.or(first).map(|visual| ArtworkImage {
        bytes: visual.data.into_vec(),
        mime: visual.media_type,
        source: ArtworkSource::Embedded,
    }))
}

/// Without the `audio` feature symphonia is not in the build: the tags
/// cannot be read. That is said, not answered with "no picture" — "I didn't
/// look" and "I found nothing" are different diagnoses (K9).
#[cfg(not(feature = "audio"))]
pub(crate) fn embedded(path: &Path) -> Result<Option<ArtworkImage>> {
    let _ = path;
    Err(Error::new(
        Stage::ArtworkRead,
        ErrorKind::Unsupported {
            provider: "local".to_owned(),
            what: "reading pictures from tags (a build without the `audio` feature)".to_owned(),
            capabilities: "NONE".to_owned(),
        },
    ))
}

/// A local file's cover: the picture in its tags, then an image in its
/// folder. The local provider answers [`crate::provider::Provider::artwork`]
/// with this, and `headshell artwork --file` calls it directly.
///
/// A folder image wins over a tag that could not be read — the user asked
/// for a cover, and there is one. With neither, the tag's error is the
/// answer, not "none": the file may be broken (K9).
///
/// # Errors
/// If the tags could not be read and there is no folder image, or the folder
/// cannot be listed.
pub(crate) fn file_cover(path: &Path) -> Result<Option<ArtworkImage>> {
    let tag_error = match embedded(path) {
        Ok(Some(image)) => return Ok(Some(image)),
        Ok(None) => None,
        Err(err) => Some(err),
    };
    match (folder(path)?, tag_error) {
        (Some((image, _)), _) => Ok(Some(image)),
        (None, Some(err)) => Err(err),
        (None, None) => Ok(None),
    }
}

/// The names a folder image goes by, in the order they are preferred.
const FOLDER_NAMES: &[&str] = &["cover", "folder", "front"];
const FOLDER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];

/// An image next to the file: `cover`, `folder` or `front`, as `.jpg`,
/// `.jpeg` or `.png`, in any case.
///
/// `Ok(None)`: there is none.
///
/// # Errors
/// If the folder cannot be listed, or the image in it cannot be read.
pub(crate) fn folder(path: &Path) -> Result<Option<(ArtworkImage, PathBuf)>> {
    let Some(dir) = path.parent() else {
        return Ok(None);
    };
    let entries =
        std::fs::read_dir(dir).map_err(|source| io_err(Stage::ArtworkRead, dir, source))?;
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in entries.flatten() {
        let candidate = entry.path();
        let (Some(stem), Some(extension)) = (
            candidate.file_stem().and_then(|s| s.to_str()),
            candidate.extension().and_then(|e| e.to_str()),
        ) else {
            continue;
        };
        let stem = stem.to_ascii_lowercase();
        let extension = extension.to_ascii_lowercase();
        let Some(rank) = FOLDER_NAMES.iter().position(|name| *name == stem) else {
            continue;
        };
        if !FOLDER_EXTENSIONS.contains(&extension.as_str()) || !candidate.is_file() {
            continue;
        }
        // Directory order differs between systems: the choice must not.
        let better = best
            .as_ref()
            .is_none_or(|(kept, kept_path)| (rank, &candidate) < (*kept, kept_path));
        if better {
            best = Some((rank, candidate));
        }
    }
    let Some((_, image)) = best else {
        return Ok(None);
    };
    let bytes =
        std::fs::read(&image).map_err(|source| io_err(Stage::ArtworkRead, &image, source))?;
    Ok(Some((
        ArtworkImage {
            bytes,
            mime: None,
            source: ArtworkSource::Folder,
        },
        image,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[cfg(feature = "audio")]
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/artwork")
            .join(name)
    }

    /// Both containers the fixtures cover: a FLAC `PICTURE` block and an ID3
    /// `APIC` frame, each marked as the front cover.
    #[cfg(feature = "audio")]
    #[test]
    fn the_front_cover_is_read_from_flac_and_id3_tags() {
        let flac = embedded(&fixture("Cover Artist - Flac Front.flac"))
            .unwrap()
            .unwrap();
        let probe = crate::artwork::image::probe(&flac.bytes).unwrap();
        assert_eq!((probe.width, probe.height), (400, 400));
        assert_eq!(flac.source, ArtworkSource::Embedded);

        let mp3 = embedded(&fixture("Cover Artist - Mp3 Front.mp3"))
            .unwrap()
            .unwrap();
        let probe = crate::artwork::image::probe(&mp3.bytes).unwrap();
        assert_eq!(probe.format, crate::artwork::image::Format::Jpeg);
        assert_eq!((probe.width, probe.height), (200, 200));
    }

    /// A file without a picture is "none", not an error.
    #[cfg(feature = "audio")]
    #[test]
    fn a_file_without_a_picture_has_none() {
        let tagged = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/audio/tagged.flac");
        assert!(embedded(&tagged).unwrap().is_none());
    }

    /// The fixtures' covers through the resize: the PNG is larger than the
    /// label, the JPEG is not — it is decoded for the thumbnail only.
    #[cfg(all(feature = "audio", feature = "artwork-resize"))]
    #[test]
    fn the_fixture_covers_resize_into_their_variants() {
        use crate::artwork::image::{Format, normalize};
        let flac = embedded(&fixture("Cover Artist - Flac Front.flac"))
            .unwrap()
            .unwrap();
        let kept = normalize(&flac.bytes).unwrap();
        assert_eq!((kept.label.width, kept.thumb.width), (320, 96));

        let mp3 = embedded(&fixture("Cover Artist - Mp3 Front.mp3"))
            .unwrap()
            .unwrap();
        let kept = normalize(&mp3.bytes).unwrap();
        assert_eq!(
            kept.label.format,
            Format::Jpeg,
            "200 px fits the label: kept as it came"
        );
        assert_eq!(kept.label.bytes, mp3.bytes);
        assert_eq!((kept.thumb.format, kept.thumb.width), (Format::Png, 96));
    }

    #[test]
    fn the_folder_image_is_found_in_any_case_and_the_order_is_fixed() {
        let dir = TempDir::new("artwork-folder");
        let track = dir.path().join("Artist - Title.flac");
        std::fs::write(&track, b"not audio").unwrap();
        assert!(folder(&track).unwrap().is_none(), "no image yet");

        std::fs::write(dir.path().join("Front.PNG"), b"front").unwrap();
        std::fs::write(dir.path().join("folder.jpg"), b"folder").unwrap();
        std::fs::write(dir.path().join("cover.txt"), b"not an image").unwrap();
        let (image, found) = folder(&track).unwrap().unwrap();
        assert_eq!(image.bytes, b"folder", "`folder` comes before `front`");
        assert_eq!(found, dir.path().join("folder.jpg"));
        assert_eq!(image.source, ArtworkSource::Folder);

        std::fs::write(dir.path().join("COVER.jpeg"), b"cover").unwrap();
        assert_eq!(folder(&track).unwrap().unwrap().0.bytes, b"cover");
    }
}
