//! Fetches Old School RuneScape's official news feed.
//!
//! Uses Jagex's own public RSS feed (`secure.runescape.com/m=news/latest_news.rss`), linked
//! directly from the official oldschool.runescape.com homepage footer. Parsed with simple
//! string operations rather than pulling in an XML crate — the feed's `<item>` fields
//! (`title`, `link`, `pubDate`, `description`, `category`) are flat, non-nested text content,
//! so a full XML parser would be overkill for reading five fields out of a fixed shape.

const NEWS_FEED_URL: &str = "https://secure.runescape.com/m=news/latest_news.rss?oldschool=true";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub pub_date: String,
    pub description: String,
    pub category: String,
    pub image_url: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum NewsError {
    #[error("network request failed: {0}")]
    Request(#[from] reqwest::Error),
}

/// Fetches the OSRS news feed and returns up to `limit` of the most recent items, in feed
/// order (newest first).
pub async fn fetch_latest_news(
    http: &reqwest::Client,
    limit: usize,
) -> Result<Vec<NewsItem>, NewsError> {
    let body = http
        .get(NEWS_FEED_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_items(&body).into_iter().take(limit).collect())
}

fn parse_items(xml: &str) -> Vec<NewsItem> {
    xml.split("<item>")
        .skip(1) // first chunk is the channel header, before any <item>
        .filter_map(|chunk| {
            let chunk = chunk.split("</item>").next().unwrap_or(chunk);
            Some(NewsItem {
                title: decode_entities(extract_tag(chunk, "title")?.trim()),
                link: decode_entities(extract_tag(chunk, "link")?.trim()),
                pub_date: decode_entities(extract_tag(chunk, "pubDate")?.trim()),
                description: decode_entities(extract_tag(chunk, "description")?.trim()),
                category: decode_entities(extract_tag(chunk, "category").unwrap_or_default().trim()),
                image_url: extract_enclosure_url(chunk),
            })
        })
        .collect()
}

/// Extracts the text between `<tag>` and `</tag>` in `chunk`, if present.
fn extract_tag(chunk: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = chunk.find(&open)? + open.len();
    let end = chunk[start..].find(&close)? + start;
    Some(chunk[start..end].to_string())
}

/// Extracts the `url="..."` attribute from a feed item's `<enclosure ... url="..." />` tag,
/// which is how this feed attaches a thumbnail image to each news item.
fn extract_enclosure_url(chunk: &str) -> Option<String> {
    let tag_start = chunk.find("<enclosure")?;
    let tag_end = chunk[tag_start..].find('>')? + tag_start;
    let tag = &chunk[tag_start..tag_end];
    let attr = "url=\"";
    let attr_start = tag.find(attr)? + attr.len();
    let attr_end = tag[attr_start..].find('"')? + attr_start;
    Some(decode_entities(&tag[attr_start..attr_end]))
}


/// Decodes the small set of XML entities actually seen in this feed. Not a general-purpose
/// HTML/XML entity decoder — just enough for news titles/descriptions.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_FEED: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
<title>Old School RuneScape Recent News</title>
<item>
<title>The Blood Moon Rises - Out Today!</title>
<dc:creator/>
<enclosure type="image/jpeg" length="0" url="https://cdn.runescape.com/thumb.jpg"/>
<description> The Blood Moon rises today, bringing the finale of the Myreque saga. </description>
<category>Game Updates</category>
<link>https://secure.runescape.com/m=news/the-blood-moon-rises---out-today?oldschool=1</link>
<pubDate>Tue, 30 Jun 2026 00:00:00 GMT</pubDate>
<guid isPermaLink="true">https://secure.runescape.com/m=news/the-blood-moon-rises---out-today?oldschool=1</guid>
</item>
<item>
<title>Wyrmscraig &amp; Friends - Unique Rewards</title>
<dc:creator/>
<enclosure type="image/png" length="0" url="https://cdn.runescape.com/thumb2.png"/>
<description> Today, we're here to talk about the rewards. </description>
<category>Community</category>
<link>https://secure.runescape.com/m=news/wyrmscraig?oldschool=1</link>
<pubDate>Fri, 26 Jun 2026 00:00:00 GMT</pubDate>
<guid isPermaLink="true">https://secure.runescape.com/m=news/wyrmscraig?oldschool=1</guid>
</item>
</channel></rss>"#;

    #[test]
    fn parses_items_in_order() {
        let items = parse_items(SAMPLE_FEED);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "The Blood Moon Rises - Out Today!");
        assert_eq!(items[0].category, "Game Updates");
        assert_eq!(
            items[0].link,
            "https://secure.runescape.com/m=news/the-blood-moon-rises---out-today?oldschool=1"
        );
        assert_eq!(items[0].pub_date, "Tue, 30 Jun 2026 00:00:00 GMT");
        assert_eq!(
            items[0].image_url.as_deref(),
            Some("https://cdn.runescape.com/thumb.jpg")
        );
        assert_eq!(items[1].title, "Wyrmscraig & Friends - Unique Rewards");
        assert_eq!(
            items[1].image_url.as_deref(),
            Some("https://cdn.runescape.com/thumb2.png")
        );
    }

    #[test]
    fn decodes_common_entities() {
        assert_eq!(decode_entities("Fish &amp; Chips"), "Fish & Chips");
        assert_eq!(decode_entities("It&#039;s here"), "It's here");
    }

    #[test]
    fn limit_truncates_results() {
        let items = parse_items(SAMPLE_FEED);
        assert_eq!(items.into_iter().take(1).count(), 1);
    }
}
