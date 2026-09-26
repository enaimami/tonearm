//! The third-party links of the chain (D-076): which release to ask for, and
//! the Cover Art Archive.

use std::sync::Arc;

use crate::error::Result;
use crate::identity::musicbrainz::{Release, default_user_agent};
use crate::identity::normalize;
use crate::ids::Mbid;
use crate::net::{HttpClient, HttpHeader, HttpRequest};

/// The public archive.
pub(crate) const DEFAULT_ARCHIVE_URL: &str = "https://coverartarchive.org";

/// What the archive keeps covers for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entity {
    Release,
    ReleaseGroup,
}

impl Entity {
    const fn segment(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::ReleaseGroup => "release-group",
        }
    }
}

/// The Cover Art Archive. It goes through [`HttpClient`] like every other
/// network call (D-020): tests hand it a fake.
pub(crate) struct CoverArtArchive {
    http: Arc<dyn HttpClient>,
    base_url: String,
    user_agent: String,
}

impl CoverArtArchive {
    pub(crate) fn new(http: Arc<dyn HttpClient>) -> Self {
        Self {
            http,
            base_url: DEFAULT_ARCHIVE_URL.to_owned(),
            user_agent: default_user_agent(),
        }
    }

    /// Another server — for tests.
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.trim_end_matches('/').to_owned();
        self
    }

    /// The front cover at 500 px. The archive answers with a redirect to
    /// where the image is kept; the client follows it.
    ///
    /// `Ok(None)`: the archive has no front cover for it (`404`) — an
    /// answer, not an error (K9).
    ///
    /// # Errors
    /// If the archive cannot be reached or answers with another error.
    pub(crate) async fn front(&self, entity: Entity, mbid: &Mbid) -> Result<Option<Vec<u8>>> {
        let url = format!(
            "{}/{}/{}/front-500",
            self.base_url,
            entity.segment(),
            mbid.as_str()
        );
        let request = HttpRequest::get(&url)
            .with_headers(vec![HttpHeader::new("User-Agent", self.user_agent.clone())]);
        let response = self.http.send(&request).await?;
        if response.status == 404 {
            return Ok(None);
        }
        response.error_for_status(&url)?;
        Ok(Some(response.body))
    }
}

/// Which release to ask the archive for.
///
/// With an album, **only** a release titled like it. A well-known song has
/// dozens of namesake recordings — on compilations, on live records — and the
/// one the identity chain matched may be on none of the album's releases:
/// its compilation's cover shown for the album would be a wrong answer that
/// looks right (K9). `None` then, and the chain searches by the album.
///
/// Among those — and without an album, among all — an official one first,
/// the artist's own (no secondary type: not a compilation, a live record or
/// a soundtrack) next, an album before an EP before a single, the earliest,
/// and the smallest MBID last: the same album gives the same cover every
/// time.
pub(crate) fn choose_release<'a>(
    releases: &'a [Release],
    album: Option<&str>,
) -> Option<&'a Release> {
    let wanted = album
        .map(normalize::normalize_text)
        .filter(|album| !album.is_empty());
    releases
        .iter()
        .filter(|release| {
            wanted
                .as_deref()
                .is_none_or(|album| normalize::normalize_text(&release.title) == album)
        })
        .min_by_key(|release| {
            (
                !release.official,
                !release.secondary_types.is_empty(),
                type_rank(release.primary_type.as_deref()),
                release.date.is_none(),
                release.date.clone().unwrap_or_default(),
                release.id.as_str().to_owned(),
            )
        })
}

fn type_rank(primary: Option<&str>) -> u8 {
    match primary {
        Some(kind) if kind.eq_ignore_ascii_case("album") => 0,
        Some(kind) if kind.eq_ignore_ascii_case("ep") => 1,
        Some(kind) if kind.eq_ignore_ascii_case("single") => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;

    fn release(id: &str, title: &str, official: bool, date: Option<&str>) -> Release {
        Release {
            id: Mbid::parse(id).unwrap(),
            title: title.to_owned(),
            official,
            date: date.map(str::to_owned),
            group: None,
            primary_type: Some("Album".to_owned()),
            secondary_types: Vec::new(),
        }
    }

    #[test]
    fn with_an_album_only_a_release_titled_like_it_is_taken() {
        let mut single = release(
            "00000000-0000-4000-8000-000000000001",
            "Creep",
            true,
            Some("1992-09-21"),
        );
        single.primary_type = Some("Single".to_owned());
        let album = release(
            "00000000-0000-4000-8000-000000000002",
            "Pablo Honey",
            true,
            Some("1993-02-22"),
        );
        let bootleg = release(
            "00000000-0000-4000-8000-000000000003",
            "Pablo Honey",
            false,
            Some("1990"),
        );
        let releases = vec![single.clone(), bootleg, album.clone()];

        assert_eq!(
            choose_release(&releases, Some("Pablo Honey (Remastered)")),
            Some(&album),
            "the edition suffix is not a different album; the official one wins"
        );
        assert_eq!(
            choose_release(&[single.clone()], Some("Pablo Honey")),
            None,
            "a release of another title is not the album's cover"
        );
        assert_eq!(choose_release(&[], Some("x")), None);
        // Without an album: the album before the single, dates aside.
        assert_eq!(choose_release(&releases, None), Some(&album));
    }

    #[test]
    fn the_artists_own_release_comes_before_a_compilation() {
        let mut compilation = release(
            "00000000-0000-4000-8000-000000000001",
            "Rencontres Trans Musicales",
            true,
            Some("1994-12-01"),
        );
        compilation.secondary_types = vec!["Compilation".to_owned()];
        let own = release(
            "00000000-0000-4000-8000-000000000002",
            "Dummy",
            true,
            Some("1994-08-22"),
        );
        let later = release(
            "00000000-0000-4000-8000-000000000003",
            "Dummy",
            true,
            Some("2014-01-01"),
        );
        let releases = vec![compilation, later, own.clone()];
        assert_eq!(choose_release(&releases, None), Some(&own));
        assert_eq!(
            choose_release(&releases, Some("dummy")),
            Some(&own),
            "the earliest"
        );
    }

    #[tokio::test]
    async fn a_missing_cover_is_an_answer_and_a_server_error_is_an_error() {
        let mbid = Mbid::parse("00000000-0000-4000-8000-000000000001").unwrap();
        let http = Arc::new(
            FakeHttp::new()
                .route_bytes(
                    "/release/00000000-0000-4000-8000-000000000001/front-500",
                    "image/jpeg",
                    b"JPEG",
                )
                .route_status("/release-group/", 404, ""),
        );
        let archive = CoverArtArchive::new(http.clone()).with_base_url("http://caa.test/");
        assert_eq!(
            archive.front(Entity::Release, &mbid).await.unwrap(),
            Some(b"JPEG".to_vec())
        );
        assert_eq!(
            archive.front(Entity::ReleaseGroup, &mbid).await.unwrap(),
            None
        );
        let user_agent = http.requests()[0]
            .headers
            .iter()
            .find(|h| h.name == "User-Agent")
            .map(|h| h.value.clone())
            .unwrap_or_default();
        assert!(user_agent.starts_with("headshell/"), "{user_agent}");

        let broken = CoverArtArchive::new(Arc::new(FakeHttp::new().route_status(
            "/release/",
            503,
            "busy",
        )))
        .with_base_url("http://caa.test");
        let err = broken.front(Entity::Release, &mbid).await.unwrap_err();
        assert!(err.chain_text().contains("503"), "{}", err.chain_text());
    }
}
