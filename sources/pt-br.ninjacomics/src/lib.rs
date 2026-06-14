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

const BASE_URL: &str = "https://ninjacomics.xyz";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

fn request(url: &str) -> core::result::Result<Request, aidoku::imports::net::RequestError> {
	Ok(Request::get(url)?.header("User-Agent", USER_AGENT))
}

fn parse_portuguese_date(date_str: &str) -> Option<i64> {
	let date_str = date_str.trim().to_lowercase();
	if date_str.is_empty() || date_str.contains("dia") || date_str.contains("semana") || date_str.contains("mês") || date_str.contains("mes") || date_str.contains("ano") {
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

	for (pt_name, num) in &months {
		if date_str.contains(pt_name) {
			let cleaned = date_str.replace(pt_name, num).replace("de ", "").replace("de", "");
			let parts: Vec<&str> = cleaned
				.split(|c: char| c.is_whitespace() || c == ',')
				.filter(|s| !s.is_empty())
				.collect();
			if parts.len() >= 3 {
				let d = parts[0];
				let m = num;
				let y = parts[2];
				let formatted = format!("{}-{}-{}", y, m, d);
				if let Some(ts) = parse_date(&formatted, "yyyy-MM-dd") {
					return Some(ts);
				}
			}
		}
	}

	parse_date(date_str, "MMMM d, yyyy")
}

fn parse_manga_list(html: &Document) -> Vec<Manga> {
	let mut entries: Vec<Manga> = Vec::new();
	if let Some(items) = html.select(".page-item-detail") {
		for item in items {
			let url = item
				.select_first(".item-thumb a")
				.and_then(|e| e.attr("href"))
				.unwrap_or_default();
			if url.is_empty() {
				continue;
			}
			let slug = url.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
			if slug.is_empty() {
				continue;
			}

			let title = item
				.select_first(".post-title h3 a, .post-title a")
				.and_then(|e| e.text())
				.unwrap_or_default();

			let cover = item
				.select_first(".item-thumb img")
				.and_then(|e| e.attr("data-src").or_else(|| e.attr("src")));

			let status = item
				.select_first(".manga-title-badges")
				.and_then(|e| e.text())
				.unwrap_or_default();

			let manga_status = if status.to_lowercase().contains("finalizada")
				|| status.to_lowercase().contains("completed")
			{
				MangaStatus::Completed
			} else {
				MangaStatus::Ongoing
			};

			entries.push(Manga {
				key: slug.clone(),
				title,
				cover,
				url: Some(format!("{}/manga/{}/", BASE_URL, slug)),
				status: manga_status,
				..Default::default()
			});
		}
	}
	entries
}

fn get_image_pages(html: &Document) -> Result<Vec<Page>> {
	let mut pages: Vec<Page> = Vec::new();

	if let Some(imgs) = html.select(".reading-content .page-break img, .reading-content img.wp-manga-chapter-img") {
		for img in imgs {
			let src = img
				.attr("data-src")
				.or_else(|| img.attr("src"))
				.unwrap_or_default();
			if !src.is_empty() {
				pages.push(Page {
					content: PageContent::Url(src, None),
					..Default::default()
				});
			}
		}
	}

	if pages.is_empty() {
		Err(AidokuError::message("no images found"))
	} else {
		Ok(pages)
	}
}

struct NinjaComics;

impl Source for NinjaComics {
	fn new() -> Self {
		NinjaComics
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		_page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		if let Some(q) = query {
			let url = format!("{}/?s={}&post_type=wp-manga", BASE_URL, q);
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
			return Err(AidokuError::message("invalid slug"));
		}

		let url = format!("{}/manga/{}/", BASE_URL, slug);
		let html = request(&url)?.html()?;
		manga.url = Some(url.clone());

		if needs_details {
			if let Some(title) = html
				.select_first(".post-title h1")
				.and_then(|e| e.text())
			{
				manga.title = title;
			}

			if let Some(cover) = html
				.select_first(".summary_image img")
				.and_then(|e| e.attr("data-src").or_else(|| e.attr("src")))
			{
				manga.cover = Some(cover);
			}

			if let Some(desc) = html
				.select_first(".description-summary .summary__content, .summary__content")
				.and_then(|e| e.text())
			{
				manga.description = Some(desc);
			}

			if let Some(items) = html.select(".post-content .post-content_item") {
				for item in items {
					let label = item.text().unwrap_or_default().to_lowercase();
					if label.contains("status") {
						if let Some(status) = item.select_first(".summary-content").and_then(|e| e.text()) {
							manga.status = match status.to_lowercase().as_str() {
								s if s.contains("ongoing") || s.contains("em dia") => MangaStatus::Ongoing,
								s if s.contains("completed") || s.contains("finalizada") => MangaStatus::Completed,
								s if s.contains("hiatus") || s.contains("hiato") => MangaStatus::Hiatus,
								s if s.contains("cancelled") || s.contains("cancelada") => MangaStatus::Cancelled,
								_ => MangaStatus::Unknown,
							};
						}
					}
				}
			}

			if let Some(genres) = html.select(".genres-content a") {
				let tags: Vec<String> = genres.filter_map(|e| e.text()).collect();
				if !tags.is_empty() {
					manga.tags = Some(tags);
				}
			}
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = Vec::new();
			if let Some(items) = html.select("li.wp-manga-chapter") {
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

					let num_text = if num_text.is_empty() {
						item.select_first("a")
							.and_then(|e| e.text())
							.unwrap_or_default()
							.trim()
							.to_string()
					} else {
						num_text
					};

					if num_text.is_empty() {
						continue;
					}

					let date_text = item
						.select_first("span.chapter-release-date i, span.chapterdate")
						.and_then(|e| e.text())
						.unwrap_or_default();

					let timestamp = parse_portuguese_date(&date_text);

					let number = {
						let mut last: Option<f32> = None;
						for part in num_text.split(|c: char| !c.is_ascii_digit() && c != '.') {
							if let Ok(n) = part.parse::<f32>() {
								last = Some(n);
							}
						}
						last
					};

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
				b.chapter_number
					.unwrap_or(0.0)
					.partial_cmp(&a.chapter_number.unwrap_or(0.0))
					.unwrap_or(core::cmp::Ordering::Equal)
			});

			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let html = request(&chapter.key)?.html()?;
		get_image_pages(&html)
	}
}

impl ListingProvider for NinjaComics {
	fn get_manga_list(&self, listing: Listing, _page: i32) -> Result<MangaPageResult> {
		let listing_id = listing.id.as_str();

		let url = match listing_id {
			"popular" => format!("{}/manga/?m_orderby=views", BASE_URL),
			"new" => format!("{}/manga/?m_orderby=new-manga", BASE_URL),
			_ => format!("{}/manga/", BASE_URL),
		};

		let html = request(&url)?.html()?;
		let entries = parse_manga_list(&html);

		Ok(MangaPageResult {
			entries,
			has_next_page: false,
		})
	}
}

impl DeepLinkHandler for NinjaComics {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if url.contains("/manga/") && !url.contains("/capitulo-") {
			let clean = url.split('?').next().unwrap_or(&url).trim_end_matches('/');
			let key = clean.rsplit('/').next().map(String::from).unwrap_or_default();
			if !key.is_empty() {
				return Ok(Some(DeepLinkResult::Manga { key }));
			}
		}
		Ok(None)
	}
}

register_source!(NinjaComics, ListingProvider, DeepLinkHandler);
