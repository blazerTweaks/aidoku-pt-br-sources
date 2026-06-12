#![no_std]
use aidoku::{
	alloc::{string::String, vec::Vec},
	helpers::uri::encode_uri_component,
	imports::net::Request,
	imports::std::parse_date,
	prelude::*,
	AidokuError, Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source,
};
use serde::Deserialize;

const BASE_URL: &str = "https://mangaflix.net";
const API_URL: &str = "https://api.mangaflix.net";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

fn request(url: &str) -> core::result::Result<Request, aidoku::imports::net::RequestError> {
	Ok(Request::get(url)?.header("User-Agent", USER_AGENT))
}

#[derive(Deserialize)]
struct MangaResponse {
	data: MangaData,
}

#[derive(Deserialize)]
struct MangaData {
	_id: String,
	name: String,
	description: Option<String>,
	poster: Option<PosterData>,
	genres: Option<Vec<GenreData>>,
	chapters: Option<Vec<ChapterData>>,
}

#[derive(Deserialize)]
struct PosterData {
	default_url: Option<String>,
}

#[derive(Deserialize)]
struct GenreData {
	_id: String,
	name: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ChapterData {
	_id: String,
	number: String,
	name: Option<String>,
	number_of_pages: Option<i32>,
	release_date: Option<String>,
}

#[derive(Deserialize)]
struct PagesResponse {
	data: PagesData,
}

#[derive(Deserialize)]
struct PagesData {
	images: Vec<ImageData>,
}

#[derive(Deserialize)]
struct ImageData {
	_id: String,
	default_url: String,
	order: i32,
}

struct MangaFlix;

impl Source for MangaFlix {
	fn new() -> Self {
		MangaFlix
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let url = if let Some(q) = query {
			format!("{}/br/browse?search={}&page={}", BASE_URL, &encode_uri_component(&q), page)
		} else {
			format!("{}/br/browse?page={}", BASE_URL, page)
		};
		println!("[MangaFlix] get_search_manga_list url={} page={}", url, page);

		let html = match request(&url) {
			Ok(req) => match req.html() {
				Ok(doc) => doc,
				Err(e) => {
					println!("[MangaFlix] html() error: {:?}", e);
					return Err(AidokuError::message(format!("html error: {:?}", e)));
				}
			},
			Err(e) => {
				println!("[MangaFlix] request() error: {:?}", e);
				return Err(AidokuError::message(format!("request error: {:?}", e)));
			}
		};

		let mut entries: Vec<Manga> = Vec::new();
		if let Some(cards) = html.select("a[href^='/br/manga/']") {
			for card in cards {
				let href = card.attr("href").unwrap_or_default();
				let key = href.rsplit('/').next().map(String::from).unwrap_or_default();
				if key.is_empty() {
					continue;
				}

				let title = card.select("span").and_then(|spans| spans.last()).and_then(|e| e.text()).unwrap_or_default();
				let cover = card
					.select_first("img")
					.and_then(|e| e.attr("src"))
					.map(|s| {
						if s.starts_with("http") {
							s
						} else {
							format!("{}{}", BASE_URL, s)
						}
					});

				entries.push(Manga {
					key: key.clone(),
					title,
					cover,
					url: Some(format!("{}/br/manga/{}", BASE_URL, &key)),
					..Default::default()
				});
			}
		}

		let has_next_page = entries.len() >= 20;
		println!("[MangaFlix] browse found {} entries, has_next={}", entries.len(), has_next_page);

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = format!("{}/v1/mangas/{}", API_URL, &manga.key);
		let response = request(&url)?.send()?;
		let data: MangaResponse = response.get_json_owned()?;
		let manga_data = data.data;

		if needs_details {
			manga.title = manga_data.name;
			manga.description = manga_data.description;
			manga.cover = manga_data.poster.and_then(|p| p.default_url);
			manga.status = MangaStatus::Ongoing;
			manga.url = Some(format!("{}/br/manga/{}", BASE_URL, &manga.key));

			let mut tags: Vec<String> = Vec::new();
			if let Some(genres) = manga_data.genres {
				for genre in genres {
					tags.push(genre.name);
				}
			}
			manga.tags = if tags.is_empty() { None } else { Some(tags) };
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = Vec::new();
			if let Some(chapter_list) = manga_data.chapters {
				for ch in chapter_list {
					let number: f32 = ch.number.parse().unwrap_or(0.0);
					let timestamp = ch
						.release_date
						.as_ref()
						.and_then(|d| parse_date(d.as_str(), "yyyy-MM-dd'T'HH:mm:ss.SSS'Z'"));

					chapters.push(Chapter {
						key: ch._id,
						chapter_number: Some(number),
						title: ch.name,
						date_uploaded: timestamp,
						..Default::default()
					});
				}
			}

			chapters.reverse();
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{}/v1/chapters/{}", API_URL, &chapter.key);
		let response = request(&url)?.send()?;
		let data: PagesResponse = response.get_json_owned()?;

		let mut pages: Vec<(i32, String)> = Vec::new();
		for img in data.data.images {
			if !img.default_url.is_empty() {
				pages.push((img.order, img.default_url));
			}
		}

		pages.sort_by(|a, b| a.0.cmp(&b.0));

		Ok(pages
			.into_iter()
			.map(|(_, url)| Page {
				content: PageContent::url(url),
				..Default::default()
			})
			.collect())
	}
}

impl ListingProvider for MangaFlix {
	fn get_manga_list(&self, _listing: Listing, page: i32) -> Result<MangaPageResult> {
		self.get_search_manga_list(None, page, Vec::new())
	}
}

impl DeepLinkHandler for MangaFlix {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if url.contains("/br/manga/") {
			let key = url.rsplit('/').next().map(String::from).unwrap_or_default();
			Ok(Some(DeepLinkResult::Manga { key }))
		} else {
			Ok(None)
		}
	}
}

register_source!(MangaFlix, ListingProvider, DeepLinkHandler);
