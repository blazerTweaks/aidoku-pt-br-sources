#![no_std]
use aidoku::{
	alloc::{string::{String, ToString}, vec::Vec},
	helpers::uri::encode_uri_component,
	imports::{
		html::{Document, Element},
		net::{Request, RequestError},
		std::{current_date, parse_date},
	},
	prelude::*,
	AidokuError, Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, Listing, ListingProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, Result, Source,
};

const BASE_URL: &str = "https://ninjacomics.xyz";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
/// Número mínimo de itens numa página para assumir que existe uma próxima.
const PAGE_SIZE_HINT: usize = 10;

fn request(url: &str) -> core::result::Result<Request, RequestError> {
	Ok(Request::get(url)?
		.header("User-Agent", USER_AGENT)
		.header("Referer", BASE_URL))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn absolute_url(url: &str) -> String {
	let url = url.trim();
	if url.starts_with("//") {
		format!("https:{}", url)
	} else if url.starts_with('/') {
		format!("{}{}", BASE_URL, url)
	} else {
		url.to_string()
	}
}

/// Lê a URL de uma imagem considerando lazy-load e espaços/quebras de linha
/// que o tema Madara costuma deixar dentro do atributo.
fn image_url(el: &Element) -> Option<String> {
	for attr in ["data-src", "data-lazy-src", "data-cfsrc", "src"] {
		if let Some(value) = el.attr(attr) {
			let value = value.trim();
			if !value.is_empty() && !value.starts_with("data:") {
				return Some(absolute_url(value));
			}
		}
	}
	None
}

fn slug_from_url(url: &str) -> String {
	url.split(['?', '#'])
		.next()
		.unwrap_or_default()
		.trim_end_matches('/')
		.rsplit('/')
		.next()
		.unwrap_or_default()
		.to_string()
}

fn parse_status(text: &str) -> MangaStatus {
	let s = text.to_lowercase();
	if s.contains("complet") || s.contains("finaliz") || s.contains("conclu") {
		MangaStatus::Completed
	} else if s.contains("hiat") {
		MangaStatus::Hiatus
	} else if s.contains("cancel") {
		MangaStatus::Cancelled
	} else if s.contains("ongoing") || s.contains("andamento") || s.contains("em dia") || s.contains("lançamento") || s.contains("ativo") {
		MangaStatus::Ongoing
	} else {
		MangaStatus::Unknown
	}
}

/// Primeiro número (aceita `.` ou `,` decimal) encontrado no texto.
fn first_number(text: &str) -> Option<f32> {
	let mut num = String::new();
	for c in text.chars() {
		if c.is_ascii_digit() {
			num.push(c);
		} else if (c == '.' || c == ',') && !num.is_empty() && !num.contains('.') {
			num.push('.');
		} else if !num.is_empty() {
			break;
		}
	}
	num.trim_end_matches('.').parse::<f32>().ok()
}

/// Número do capítulo: prioriza o número logo após "capítulo"/"cap"/"chapter".
fn parse_chapter_number(text: &str) -> Option<f32> {
	let lower = text.to_lowercase();
	for key in ["capítulo", "capitulo", "cap.", "cap", "chapter", "ch."] {
		if let Some(pos) = lower.find(key) {
			if let Some(n) = first_number(&lower[pos + key.len()..]) {
				return Some(n);
			}
		}
	}
	first_number(&lower)
}

const MONTHS: [(&str, u32); 19] = [
	("jan", 1), ("fev", 2), ("feb", 2), ("mar", 3), ("abr", 4), ("apr", 4),
	("mai", 5), ("may", 5), ("jun", 6), ("jul", 7), ("ago", 8), ("aug", 8),
	("set", 9), ("sep", 9), ("out", 10), ("oct", 10), ("nov", 11), ("dez", 12), ("dec", 12),
];

/// Datas absolutas: "6 de janeiro de 2026", "janeiro 6, 2026", "06/01/2026", "2026-01-06".
fn parse_absolute_date(s: &str) -> Option<i64> {
	let mut month: Option<u32> = None;
	let mut numbers: Vec<&str> = Vec::new();
	for token in s.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
		if token.chars().all(|c| c.is_ascii_digit()) {
			numbers.push(token);
		} else if month.is_none() {
			month = MONTHS
				.iter()
				.find(|(prefix, _)| token.starts_with(prefix))
				.map(|(_, m)| *m);
		}
	}

	let (y, m, d) = if let Some(m) = month {
		let year = numbers.iter().find(|n| n.len() == 4)?;
		let day = numbers.iter().find(|n| n.len() <= 2)?;
		(year.parse::<u32>().ok()?, m, day.parse::<u32>().ok()?)
	} else if numbers.len() >= 3 {
		if numbers[0].len() == 4 {
			(numbers[0].parse().ok()?, numbers[1].parse().ok()?, numbers[2].parse().ok()?)
		} else if numbers[2].len() == 4 {
			(numbers[2].parse().ok()?, numbers[1].parse().ok()?, numbers[0].parse().ok()?)
		} else {
			return None;
		}
	} else {
		return None;
	};

	if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
		return None;
	}
	parse_date(format!("{:04}-{:02}-{:02}", y, m, d), "yyyy-MM-dd")
}

/// Datas relativas: "há 2 dias", "3 horas atrás", "ontem", "2 days ago".
fn parse_relative_date(s: &str) -> Option<i64> {
	let now = current_date();
	if s.contains("hoje") || s.contains("agora") || s.contains("today") || s.contains("just now") {
		return Some(now);
	}
	if s.contains("ontem") || s.contains("yesterday") {
		return Some(now - 86_400);
	}
	let amount: i64 = match first_number(s) {
		Some(n) => n as i64,
		None if s.contains(" um ") || s.contains(" uma ") || s.starts_with("um ") || s.starts_with("uma ") || s.contains(" an ") || s.contains(" a ") => 1,
		None => return None,
	};
	let unit: i64 = if s.contains("seg") || s.contains("sec") {
		1
	} else if s.contains("min") {
		60
	} else if s.contains("hora") || s.contains("hour") {
		3_600
	} else if s.contains("dia") || s.contains("day") {
		86_400
	} else if s.contains("semana") || s.contains("week") {
		604_800
	} else if s.contains("mês") || s.contains("mes") || s.contains("month") {
		2_592_000
	} else if s.contains("ano") || s.contains("year") {
		31_536_000
	} else {
		return None;
	};
	Some(now - amount * unit)
}

fn parse_pt_date(text: &str) -> Option<i64> {
	let s = text.trim().to_lowercase();
	if s.is_empty() {
		return None;
	}
	parse_absolute_date(&s).or_else(|| parse_relative_date(&s))
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Lista de mangás das páginas de listagem (`.page-item-detail`) e de busca (`.c-tabs-item__content`).
fn parse_manga_list(html: &Document) -> Vec<Manga> {
	let mut entries: Vec<Manga> = Vec::new();
	let Some(items) = html.select(".page-item-detail, .c-tabs-item__content") else {
		return entries;
	};
	for item in items {
		let Some(link) = item.select_first(".item-thumb a, .tab-thumb a, .post-title a") else {
			continue;
		};
		let url = link.attr("href").unwrap_or_default();
		let slug = slug_from_url(&url);
		if slug.is_empty() || entries.iter().any(|m| m.key == slug) {
			continue;
		}

		let title = item
			.select_first(".post-title h3 a, .post-title h4 a, .post-title a")
			.and_then(|e| e.text())
			.or_else(|| link.attr("title"))
			.unwrap_or_default();

		let cover = item
			.select_first(".item-thumb img, .tab-thumb img, img")
			.and_then(|e| image_url(&e));

		let status = item
			.select_first(".manga-title-badges, .mg_status .summary-content")
			.and_then(|e| e.text())
			.map(|s| parse_status(&s))
			.unwrap_or(MangaStatus::Unknown);

		entries.push(Manga {
			key: slug.clone(),
			title,
			cover,
			url: Some(format!("{}/manga/{}/", BASE_URL, slug)),
			status,
			..Default::default()
		});
	}
	entries
}

fn has_next_page(html: &Document, entries: &[Manga]) -> bool {
	html.select_first(".nav-previous a, a.nextpostslink, a.next.page-numbers, #navigation-ajax")
		.is_some()
		|| entries.len() >= PAGE_SIZE_HINT
}

fn fetch_manga_list(url: &str, page: i32) -> Result<MangaPageResult> {
	let html = match request(url)?.html() {
		Ok(html) => html,
		// Páginas além da última costumam devolver 404: encerra a paginação em vez de falhar.
		Err(_) if page > 1 => {
			return Ok(MangaPageResult {
				entries: Vec::new(),
				has_next_page: false,
			});
		}
		Err(e) => return Err(e.into()),
	};
	let entries = parse_manga_list(&html);
	let has_next_page = !entries.is_empty() && has_next_page(&html, &entries);
	Ok(MangaPageResult {
		entries,
		has_next_page,
	})
}

fn has_chapters(doc: &Document) -> bool {
	doc.select("li.wp-manga-chapter").is_some_and(|l| !l.is_empty())
}

/// Temas Madara recentes carregam a lista de capítulos via AJAX; tenta os dois endpoints conhecidos.
fn load_chapter_document(manga_url: &str, html: Document) -> Document {
	if has_chapters(&html) {
		return html;
	}

	let ajax = Request::post(format!("{}ajax/chapters/", manga_url)).map(|r| {
		r.header("User-Agent", USER_AGENT)
			.header("Referer", manga_url)
			.header("X-Requested-With", "XMLHttpRequest")
	});
	if let Ok(doc) = ajax.and_then(|r| r.html()) {
		if has_chapters(&doc) {
			return doc;
		}
	}

	let manga_id = html
		.select_first("#manga-chapters-holder")
		.and_then(|e| e.attr("data-id"))
		.or_else(|| html.select_first(".rating-post-id").and_then(|e| e.attr("value")))
		.or_else(|| html.select_first("input.rating-post-id").and_then(|e| e.attr("value")));
	if let Some(id) = manga_id {
		let body = format!("action=manga_get_chapters&manga={}", id.trim());
		let legacy = Request::post(format!("{}/wp-admin/admin-ajax.php", BASE_URL)).map(|r| {
			r.header("User-Agent", USER_AGENT)
				.header("Referer", manga_url)
				.header("X-Requested-With", "XMLHttpRequest")
				.header("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
				.body(body.as_bytes())
		});
		if let Ok(doc) = legacy.and_then(|r| r.html()) {
			if has_chapters(&doc) {
				return doc;
			}
		}
	}

	html
}

fn parse_chapters(doc: &Document) -> Vec<Chapter> {
	let mut chapters: Vec<Chapter> = Vec::new();
	let Some(items) = doc.select("li.wp-manga-chapter") else {
		return chapters;
	};
	for item in items {
		let Some(link) = item.select_first("a") else {
			continue;
		};
		let href = link.attr("href").map(|h| absolute_url(&h)).unwrap_or_default();
		if href.is_empty() {
			continue;
		}

		let title = item
			.select_first("span.chapternum")
			.and_then(|e| e.text())
			.or_else(|| link.text())
			.unwrap_or_default()
			.trim()
			.to_string();
		if title.is_empty() {
			continue;
		}

		let date_uploaded = item
			.select_first("span.chapter-release-date i, span.chapter-release-date, span.chapterdate")
			.and_then(|e| e.text())
			.and_then(|d| parse_pt_date(&d))
			.or_else(|| {
				// Capítulos novos trazem a data só no `title` do selo "NEW".
				item.select_first("span.chapter-release-date a")
					.and_then(|e| e.attr("title"))
					.and_then(|d| parse_pt_date(&d))
			});

		chapters.push(Chapter {
			key: href.clone(),
			chapter_number: parse_chapter_number(&title),
			title: Some(title),
			date_uploaded,
			url: Some(href),
			..Default::default()
		});
	}

	chapters.sort_by(|a, b| {
		b.chapter_number
			.unwrap_or(-1.0)
			.partial_cmp(&a.chapter_number.unwrap_or(-1.0))
			.unwrap_or(core::cmp::Ordering::Equal)
	});
	chapters
}

fn get_image_pages(html: &Document) -> Result<Vec<Page>> {
	let mut pages: Vec<Page> = Vec::new();
	if let Some(imgs) = html.select(".reading-content .page-break img, .reading-content img.wp-manga-chapter-img") {
		for img in imgs {
			if let Some(src) = image_url(&img) {
				if !pages.iter().any(|p| matches!(&p.content, PageContent::Url(u, _) if *u == src)) {
					pages.push(Page {
						content: PageContent::Url(src, None),
						..Default::default()
					});
				}
			}
		}
	}

	if pages.is_empty() {
		Err(AidokuError::message("no images found"))
	} else {
		Ok(pages)
	}
}

// ---------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------

struct NinjaComics;

impl Source for NinjaComics {
	fn new() -> Self {
		NinjaComics
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let Some(q) = query.filter(|q| !q.trim().is_empty()) else {
			return self.get_manga_list(
				Listing {
					id: "latest".into(),
					..Default::default()
				},
				page,
			);
		};
		let page_path = if page > 1 { format!("/page/{}", page) } else { String::new() };
		let url = format!(
			"{}{}/?s={}&post_type=wp-manga",
			BASE_URL,
			page_path,
			encode_uri_component(q.trim())
		);
		fetch_manga_list(&url, page)
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let slug = if !manga.key.is_empty() {
			manga.key.clone()
		} else {
			manga.url.as_deref().map(slug_from_url).unwrap_or_default()
		};
		if slug.is_empty() {
			return Err(AidokuError::message("invalid slug"));
		}

		let url = format!("{}/manga/{}/", BASE_URL, slug);
		let html = request(&url)?.html()?;
		manga.url = Some(url.clone());

		if needs_details {
			if let Some(title) = html.select_first(".post-title h1").and_then(|e| e.text()) {
				manga.title = title;
			}

			if let Some(cover) = html.select_first(".summary_image img").and_then(|e| image_url(&e)) {
				manga.cover = Some(cover);
			}

			if let Some(desc) = html
				.select_first(".description-summary .summary__content, .summary__content, .manga-excerpt")
				.and_then(|e| e.text())
			{
				manga.description = Some(desc);
			}

			if let Some(authors) = html.select(".author-content a") {
				let authors: Vec<String> = authors.filter_map(|e| e.text()).collect();
				if !authors.is_empty() {
					manga.authors = Some(authors);
				}
			}

			if let Some(artists) = html.select(".artist-content a") {
				let artists: Vec<String> = artists.filter_map(|e| e.text()).collect();
				if !artists.is_empty() {
					manga.artists = Some(artists);
				}
			}

			if let Some(items) = html.select(".post-content .post-content_item, .post-status .post-content_item") {
				for item in items {
					let label = item
						.select_first(".summary-heading")
						.and_then(|e| e.text())
						.unwrap_or_default()
						.to_lowercase();
					if label.contains("status") {
						if let Some(status) = item.select_first(".summary-content").and_then(|e| e.text()) {
							manga.status = parse_status(&status);
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
			let doc = load_chapter_document(&url, html);
			manga.chapters = Some(parse_chapters(&doc));
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// `style=list` garante todas as imagens numa única página.
		let base = chapter.key.split('?').next().unwrap_or(&chapter.key);
		let url = format!("{}?style=list", base);
		let html = request(&url)?.html()?;
		get_image_pages(&html)
	}
}

impl ListingProvider for NinjaComics {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let order = match listing.id.as_str() {
			"popular" => "views",
			"new" => "new-manga",
			_ => "latest",
		};
		let page_path = if page > 1 { format!("page/{}/", page) } else { String::new() };
		let url = format!("{}/manga/{}?m_orderby={}", BASE_URL, page_path, order);
		fetch_manga_list(&url, page)
	}
}

impl DeepLinkHandler for NinjaComics {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = url.split(['?', '#']).next().unwrap_or_default();
		let Some(pos) = path.find("/manga/") else {
			return Ok(None);
		};
		let segments: Vec<&str> = path[pos + "/manga/".len()..]
			.split('/')
			.filter(|s| !s.is_empty())
			.collect();
		match segments.as_slice() {
			[slug] => Ok(Some(DeepLinkResult::Manga {
				key: slug.to_string(),
			})),
			[slug, chapter, ..] => Ok(Some(DeepLinkResult::Chapter {
				manga_key: slug.to_string(),
				key: format!("{}/manga/{}/{}/", BASE_URL, slug, chapter),
			})),
			_ => Ok(None),
		}
	}
}

register_source!(NinjaComics, ListingProvider, DeepLinkHandler);
