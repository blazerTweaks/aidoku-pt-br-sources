#![no_std]
use aidoku::{
	alloc::{string::{String, ToString}, vec::Vec},
	imports::{
		html::Document,
		net::Request,
		std::parse_date,
	},
	prelude::*,
	AidokuError, Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source,
};

const BASE_URL: &str = "https://tsundoku.com.br";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const CATEGORIES: &[(&str, &str)] = &[
	("mangas", "/mangas/"),
	("novels", "/novels/"),
	("manhwas", "/manhwas/"),
	("manhuas", "/manhuas/"),
];

fn request(url: &str) -> core::result::Result<Request, aidoku::imports::net::RequestError> {
	Ok(Request::get(url)?.header("User-Agent", USER_AGENT))
}

fn parse_manga_list(html: &Document) -> Vec<Manga> {
	let mut entries: Vec<Manga> = Vec::new();
	if let Some(cards) = html.select(".listupd .bs") {
		for card in cards {
			let url = card
				.select_first(".bsx a")
				.and_then(|e| e.attr("href"))
				.unwrap_or_default();
			if url.is_empty() {
				continue;
			}
			let slug = url.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
			if slug.is_empty() {
				continue;
			}

			let title = card
				.select_first(".bigor .tt")
				.and_then(|e| e.text())
				.unwrap_or_default();

			let cover = card
				.select_first(".limit img")
				.and_then(|e| e.attr("src"));

			let manga_type = card
				.select_first(".limit .typename, .limit .novelabel")
				.and_then(|e| e.text())
				.unwrap_or_default()
				.trim()
				.to_string();

			let status = card
				.select_first(".limit .status")
				.and_then(|e| e.text())
				.unwrap_or_default()
				.trim()
				.to_string();

			let manga_status = if status.contains("Completed") {
				MangaStatus::Completed
			} else {
				MangaStatus::Ongoing
			};

			let url_with_type = if !manga_type.is_empty() {
				format!("{}/manga/{}/?type={}", BASE_URL, slug, manga_type)
			} else {
				format!("{}/manga/{}/", BASE_URL, slug)
			};

			entries.push(Manga {
				key: slug,
				title,
				cover,
				url: Some(url_with_type),
				status: manga_status,
				..Default::default()
			});
		}
	}
	entries
}

fn parse_portuguese_date(date_str: &str) -> Option<i64> {
	let date_str = date_str.trim();
	if date_str.is_empty() {
		return None;
	}

	let months = [
		("janeiro", "01"),
		("fevereiro", "02"),
		("março", "03"),
		("abril", "04"),
		("maio", "05"),
		("junho", "06"),
		("julho", "07"),
		("agosto", "08"),
		("setembro", "09"),
		("outubro", "10"),
		("novembro", "11"),
		("dezembro", "12"),
	];

	let lower = date_str.to_lowercase();
	for (pt_name, num) in &months {
		if lower.contains(pt_name) {
			let normalized = lower.replace(pt_name, num);
			let parts: Vec<&str> = normalized
				.split(|c| c == ' ' || c == ',')
				.filter(|s| !s.is_empty())
				.collect();
			if parts.len() >= 3 {
				// Formato esperado: "15 janeiro 2024" → parts = ["15", "01", "2024"]
				let d = parts[0];
				let m = num; // já convertido
				let y = parts[2];
				let formatted = format!("{}-{}-{}", y, m, d);
				return parse_date(&formatted, "yyyy-MM-dd");
			}
		}
	}

	parse_date(date_str, "MMMM d, yyyy")
}

fn parse_chapter_number(num_text: &str) -> Option<f32> {
	let num_text = num_text.trim();
	if num_text.is_empty() {
		return None;
	}
	let mut last_num: Option<f32> = None;
	for part in num_text.split(|c: char| !c.is_ascii_digit() && c != '.') {
		if let Ok(n) = part.parse::<f32>() {
			last_num = Some(n);
		}
	}
	last_num
}

fn get_image_pages(html: &Document) -> Result<Vec<Page>> {
	let mut pages: Vec<Page> = Vec::new();

	if let Some(imgs) = html.select("#readerarea img") {
		for img in imgs {
			if let Some(src) = img.attr("src") {
				if !src.is_empty() && !src.contains("readerarea.svg") {
					pages.push(Page {
						content: PageContent::Url(src, None),
						..Default::default()
					});
				}
			}
		}
	}

	if pages.is_empty() {
		return Err(AidokuError::message("no images found"));
	}

	Ok(pages)
}

fn get_novel_pages(html: &Document) -> Result<Vec<Page>> {
	let mut pages: Vec<Page> = Vec::new();

	if let Some(imgs) = html.select("#readerarea img") {
		let mut has_images = false;
		for img in imgs {
			if let Some(src) = img.attr("src") {
				if !src.is_empty() && !src.contains("readerarea.svg") && !src.contains("apoiar") {
					pages.push(Page {
						content: PageContent::Url(src, None),
						..Default::default()
					});
					has_images = true;
				}
			}
		}
		if has_images {
			return Ok(pages);
		}
	}

	if let Some(content) = html.select_first(".entry-content-single").and_then(|e| e.text()) {
		let clean = content.trim().to_string();
		if !clean.is_empty() {
			pages.push(Page {
				content: PageContent::Text(clean),
				..Default::default()
			});
			return Ok(pages);
		}
	}

	Err(AidokuError::message("no novel content found"))
}

struct Tsundoku;

impl Source for Tsundoku {
	fn new() -> Self {
		Tsundoku
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		_page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		if let Some(q) = query {
			let url = format!("{}/?s={}", BASE_URL, q);
			let html = request(&url)?.html()?;
			let entries = parse_manga_list(&html);
			Ok(MangaPageResult {
				entries,
				has_next_page: false,
			})
		} else {
			Ok(MangaPageResult {
				entries: Vec::new(),
				has_next_page: false,
			})
		}
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = if !manga.key.is_empty() {
			manga.key.clone()
		} else if let Some(ref url) = manga.url {
			url.trim_end_matches('/')
				.rsplit('/')
				.next()
				.map(String::from)
				.unwrap_or_default()
		} else {
			String::new()
		};

		if slug.is_empty() {
			return Err(AidokuError::message("invalid manga slug"));
		}

		let url = format!("{}/manga/{}/", BASE_URL, slug);
		let html = request(&url)?.html()?;

		manga.url = Some(url.clone());

		if needs_details {
			if let Some(title) = html
				.select_first("h1.entry-title")
				.and_then(|e| e.text())
			{
				manga.title = title;
			}

			if let Some(cover) = html
				.select_first("div.thumb img")
				.and_then(|e| e.attr("src"))
			{
				manga.cover = Some(cover);
			}

			if let Some(desc) = html
				.select_first("div[itemprop=\"description\"]")
				.and_then(|e| e.text())
			{
				manga.description = Some(desc);
			}

			let mut manga_type = String::new();
			if let Some(items) = html.select(".tsinfo .imptdt") {
				for item in items {
					let label = item.text().unwrap_or_default();
					if label.contains("Status") {
						if let Some(status) = item.select_first("i").and_then(|e| e.text()) {
							manga.status = match status.to_lowercase().as_str() {
								s if s.contains("ongoing") => MangaStatus::Ongoing,
								s if s.contains("completed") => MangaStatus::Completed,
								_ => MangaStatus::Unknown,
							};
						}
					} else if label.contains("Tipo") {
						if let Some(t) = item.select_first("a").and_then(|e| e.text()) {
							manga_type = t;
						}
					}
				}
			}

			if let Some(genres) = html.select("span.mgen a") {
				let tags: Vec<String> = genres.filter_map(|e| e.text()).collect();
				if !tags.is_empty() {
					manga.tags = Some(tags);
				}
			}

			if !manga_type.is_empty() {
				manga.url = Some(format!("{}?type={}", url, manga_type));
			}
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = Vec::new();
			if let Some(items) = html.select("#chapterlist ul li") {
				for item in items {
					let href = item
						.select_first("a")
						.and_then(|e| e.attr("href"))
						.unwrap_or_default();
					if href.is_empty() {
						continue;
					}

					let num_text = item
						.select_first("span.chapternum")
						.and_then(|e| e.text())
						.unwrap_or_default()
						.trim()
						.to_string();

					if num_text.is_empty() {
						continue;
					}

					let date_text = item
						.select_first("span.chapterdate")
						.and_then(|e| e.text())
						.unwrap_or_default();

					let timestamp = parse_portuguese_date(&date_text);
					let number = parse_chapter_number(&num_text);

					chapters.push(Chapter {
						key: href,
						chapter_number: number,
						title: Some(num_text),
						date_uploaded: timestamp,
						..Default::default()
					});
				}
			}

			chapters.sort_by(|a, b| {
				a.chapter_number
					.unwrap_or(0.0)
					.partial_cmp(&b.chapter_number.unwrap_or(0.0))
					.unwrap_or(core::cmp::Ordering::Equal)
			});

			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let chapter_url = &chapter.key;
		let html = request(chapter_url)?.html()?;

		let is_novel = manga
			.url
			.as_ref()
			.map_or(false, |u| u.contains("?type=Novel"));

		if is_novel {
			get_novel_pages(&html)
		} else {
			get_image_pages(&html)
		}
	}
}

impl ListingProvider for Tsundoku {
	fn get_manga_list(&self, listing: Listing, _page: i32) -> Result<MangaPageResult> {
		// listing.id é String diretamente (não Option<String>)
		let listing_id = listing.id.as_str();

		let mut all_entries: Vec<Manga> = Vec::new();
		let mut seen_keys: Vec<String> = Vec::new();

		if listing_id == "todos" {
			for (_name, path) in CATEGORIES {
				let url = format!("{}{}", BASE_URL, path);
				if let Ok(html) = request(&url).and_then(|r| r.html().map_err(Into::into)) {
					for manga in parse_manga_list(&html) {
						if !seen_keys.contains(&manga.key) {
							seen_keys.push(manga.key.clone());
							all_entries.push(manga);
						}
					}
				}
			}
		} else {
			let path = CATEGORIES
				.iter()
				.find(|(id, _)| *id == listing_id)
				.map(|(_, path)| *path)
				.unwrap_or("/mangas/");
			let url = format!("{}{}", BASE_URL, path);
			if let Ok(html) = request(&url).and_then(|r| r.html().map_err(Into::into)) {
				all_entries = parse_manga_list(&html);
			}
		}

		Ok(MangaPageResult {
			entries: all_entries,
			has_next_page: false,
		})
	}
}

impl DeepLinkHandler for Tsundoku {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if url.contains("/manga/") {
			let clean = url.split('?').next().unwrap_or(&url).trim_end_matches('/');
			let key = clean.rsplit('/').next().map(String::from).unwrap_or_default();
			if !key.is_empty() {
				return Ok(Some(DeepLinkResult::Manga { key }));
			}
		}
		Ok(None)
	}
}

register_source!(Tsundoku, ListingProvider, DeepLinkHandler);