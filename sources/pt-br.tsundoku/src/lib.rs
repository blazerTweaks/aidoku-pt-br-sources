#![no_std]
use aidoku::{
	alloc::{string::{String, ToString}, vec, vec::Vec},
	helpers::uri::encode_uri_component,
	imports::{
		html::{Document, Element},
		net::Request,
		std::{current_date, parse_date},
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
				.and_then(|e| image_url(&e));

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
				format!("{}/manga/{}/?type={}", BASE_URL, slug, encode_uri_component(&manga_type))
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

/// Lê a URL de uma imagem considerando lazy-load e espaços dentro do atributo.
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

/// Datas absolutas: "janeiro 6, 2026", "6 de janeiro de 2026", "06/01/2026", "2026-01-06".
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

fn parse_portuguese_date(text: &str) -> Option<i64> {
	let s = text.trim().to_lowercase();
	if s.is_empty() {
		return None;
	}
	parse_absolute_date(&s).or_else(|| parse_relative_date(&s))
}

fn has_next_page(html: &Document) -> bool {
	html.select_first(".hpage a.r, .pagination a.next, a.next.page-numbers")
		.is_some()
}

fn is_novel_url(url: &str) -> bool {
	url.to_lowercase().contains("type=") && url.to_lowercase().contains("novel")
}

fn extract_float_after(text: &str, prefix: &str) -> Option<f32> {
	if let Some(pos) = text.find(prefix) {
		let after = text[pos + prefix.len()..].trim_start();
		let mut num_str = String::new();
		for c in after.chars() {
			if c.is_ascii_digit() || c == '.' {
				num_str.push(c);
			} else if !num_str.is_empty() {
				break;
			}
		}
		if !num_str.is_empty() {
			return num_str.parse::<f32>().ok();
		}
	}
	None
}

fn parse_chapter_metadata(num_text: &str) -> Option<(Option<f32>, f32, String)> {
	let num_text = num_text.trim();
	if num_text.is_empty() {
		return None;
	}

	let lower = num_text.to_lowercase();

	if lower.contains("posfácio") || lower.contains("posf.") {
		return None;
	}

	if lower.contains("vol.") {
		let volume = extract_float_after(&lower, "vol.");
		let chapter = extract_float_after(&lower, "cap.");

		let ch_base = chapter.unwrap_or(0.0);
		let is_ilust = lower.contains("ilust");
		let ch_num = if is_ilust { ch_base + 0.0001 } else { ch_base };

		Some((volume, ch_num, num_text.to_string()))
	} else if lower.contains("cap.") {
		let chapter = extract_float_after(&lower, "cap.");
		chapter.map(|ch| (None, ch, format!("Cap. {}", ch)))
	} else {
		parse_chapter_number(num_text).map(|n| (None, n, num_text.to_string()))
	}
}

fn is_content_img(src: &str) -> bool {
	!src.is_empty()
		&& !src.contains("readerarea.svg")
		&& !src.contains("apoiar")
		&& !src.ends_with(".svg")
}

fn get_image_pages(html: &Document) -> Result<Vec<Page>> {
	let mut pages: Vec<Page> = Vec::new();

	if let Some(imgs) = html.select("#readerarea img") {
		for img in imgs {
			if let Some(src) = image_url(&img) {
				if is_content_img(&src) {
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
	if let Some(content) = html.select_first("#readerarea").and_then(|e| e.text()) {
		let clean = content.trim();
		let clean = if let Some(pos) = clean.find("Agradecimentos") {
			clean[..pos].trim()
		} else {
			clean
		};
		if clean.len() > 50 {
			return Ok(vec![Page {
				content: PageContent::Text(clean.to_string()),
				..Default::default()
			}]);
		}
	}
	get_image_pages(html)
}

struct Tsundoku;

impl Source for Tsundoku {
	fn new() -> Self {
		Tsundoku
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let Some(q) = query.filter(|q| !q.trim().is_empty()) else {
			return Ok(MangaPageResult {
				entries: Vec::new(),
				has_next_page: false,
			});
		};
		let page_path = if page > 1 { format!("/page/{}", page) } else { String::new() };
		let url = format!("{}{}/?s={}", BASE_URL, page_path, encode_uri_component(q.trim()));
		let html = match request(&url)?.html() {
			Ok(html) => html,
			Err(_) if page > 1 => {
				return Ok(MangaPageResult {
					entries: Vec::new(),
					has_next_page: false,
				});
			}
			Err(e) => return Err(e.into()),
		};
		let entries = parse_manga_list(&html);
		let has_next_page = !entries.is_empty() && has_next_page(&html);
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
				.and_then(|e| image_url(&e))
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
				manga.url = Some(format!("{}?type={}", url, encode_uri_component(&manga_type)));
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

					let raw = num_text.clone();

					let Some((volume, chapter_number, title)) = parse_chapter_metadata(&num_text) else {
						continue;
					};

					let date_text = item
						.select_first("span.chapterdate")
						.and_then(|e| e.text())
						.unwrap_or_default();

					let timestamp = parse_portuguese_date(&date_text);

					chapters.push(Chapter {
						key: href.clone(),
						volume_number: volume,
						chapter_number: Some(chapter_number),
						title: Some(title.clone()),
						date_uploaded: timestamp,
						..Default::default()
					});

					let lower = raw.to_lowercase();
					if lower.contains("vol.") && !lower.contains("ilust") {
						chapters.push(Chapter {
							key: format!("{}#images", href),
							volume_number: volume,
							chapter_number: Some(chapter_number + 0.00005),
							title: Some(format!("{} (Ilust.)", title)),
							date_uploaded: timestamp,
							..Default::default()
						});
					}
				}
			}

			chapters.sort_by(|a, b| {
				let a_vol = a.volume_number.unwrap_or(f32::MAX);
				let b_vol = b.volume_number.unwrap_or(f32::MAX);
				match b_vol.partial_cmp(&a_vol).unwrap_or(core::cmp::Ordering::Equal) {
					core::cmp::Ordering::Equal => b
						.chapter_number
						.unwrap_or(0.0)
						.partial_cmp(&a.chapter_number.unwrap_or(0.0))
						.unwrap_or(core::cmp::Ordering::Equal),
					ord => ord,
				}
			});

			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let chapter_url = &chapter.key;

		if let Some(pos) = chapter_url.rfind("#images") {
			let original_url = &chapter_url[..pos];
			return get_image_pages(&request(original_url)?.html()?);
		}

		let html = request(chapter_url)?.html()?;

		let is_novel = manga.url.as_deref().is_some_and(is_novel_url);

		if is_novel {
			get_novel_pages(&html)
		} else {
			get_image_pages(&html).or_else(|_| get_novel_pages(&html))
		}
	}
}

impl ListingProvider for Tsundoku {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let listing_id = listing.id.as_str();
		let paths: Vec<&str> = if listing_id == "todos" {
			CATEGORIES.iter().map(|(_, path)| *path).collect()
		} else {
			vec![CATEGORIES
				.iter()
				.find(|(id, _)| *id == listing_id)
				.map(|(_, path)| *path)
				.unwrap_or("/mangas/")]
		};

		let mut all_entries: Vec<Manga> = Vec::new();
		let mut has_next = false;
		let mut last_error: Option<AidokuError> = None;
		let mut any_ok = false;

		for path in paths {
			let url = if page > 1 {
				format!("{}{}page/{}/", BASE_URL, path, page)
			} else {
				format!("{}{}", BASE_URL, path)
			};
			match request(&url).and_then(|r| r.html()) {
				Ok(html) => {
					any_ok = true;
					has_next |= has_next_page(&html);
					for manga in parse_manga_list(&html) {
						if !all_entries.iter().any(|m| m.key == manga.key) {
							all_entries.push(manga);
						}
					}
				}
				Err(e) => last_error = Some(e.into()),
			}
		}

		// Só propaga o erro (ex.: Cloudflare) se nenhuma categoria carregou na 1ª página;
		// em páginas seguintes, falha = fim da paginação.
		if !any_ok && page <= 1 {
			if let Some(e) = last_error {
				return Err(e);
			}
		}

		Ok(MangaPageResult {
			has_next_page: has_next && !all_entries.is_empty(),
			entries: all_entries,
		})
	}
}

impl DeepLinkHandler for Tsundoku {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if url.contains("/manga/") {
			let clean = url.split(['?', '#']).next().unwrap_or(&url).trim_end_matches('/');
			let key = clean.rsplit('/').next().map(String::from).unwrap_or_default();
			if !key.is_empty() {
				return Ok(Some(DeepLinkResult::Manga { key }));
			}
		}
		Ok(None)
	}
}

register_source!(Tsundoku, ListingProvider, DeepLinkHandler);