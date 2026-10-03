"""Source providers that know how to detect updates for different source types."""

from __future__ import annotations

from abc import ABC, abstractmethod
from datetime import datetime, timezone
import email.utils
import json
import subprocess
from typing import Any
from urllib.parse import parse_qs, quote_plus, urlparse
from xml.etree import ElementTree

import requests

from .models import CheckState, SourceSubscription


class SourceProvider(ABC):
    """Interface for checking one source type for updates."""

    @abstractmethod
    def supports(self, source_type: str) -> bool:
        """Return True when this provider handles the source type."""

    @abstractmethod
    def check_for_update(
        self, subscription: SourceSubscription, state: CheckState
    ) -> tuple[dict[str, Any] | None, CheckState]:
        """Return update payload if a new update exists and the next state."""


class WebsiteProvider(SourceProvider):
    """Detect website updates from ETag / Last-Modified headers."""

    def supports(self, source_type: str) -> bool:
        return source_type == "website"

    def check_for_update(
        self, subscription: SourceSubscription, state: CheckState
    ) -> tuple[dict[str, Any] | None, CheckState]:
        headers: dict[str, str] = {}
        if state.etag:
            headers["If-None-Match"] = state.etag
        if state.last_modified:
            headers["If-Modified-Since"] = state.last_modified

        response = requests.head(subscription.source_url, headers=headers, timeout=15)
        if response.status_code == 304:
            return None, state

        response.raise_for_status()
        next_state = CheckState(
            last_update_id=response.headers.get("ETag") or response.headers.get("Last-Modified"),
            etag=response.headers.get("ETag"),
            last_modified=response.headers.get("Last-Modified"),
        )
        update_id = next_state.last_update_id
        if update_id is None or update_id == state.last_update_id:
            return None, next_state

        return (
            {
                "update_id": update_id,
                "title": subscription.source_url,
                "published_at": _parse_http_date(next_state.last_modified),
                "payload": {
                    "etag": next_state.etag,
                    "last_modified": next_state.last_modified,
                },
            },
            next_state,
        )


class RssProvider(SourceProvider):
    """Detect updates by tracking the most recent RSS/Atom entry id."""

    def supports(self, source_type: str) -> bool:
        return source_type in {"rss", "blog"}

    def check_for_update(
        self, subscription: SourceSubscription, state: CheckState
    ) -> tuple[dict[str, Any] | None, CheckState]:
        response = requests.get(subscription.source_url, timeout=20)
        response.raise_for_status()

        root = ElementTree.fromstring(response.text)
        item = _extract_first_feed_item(root)
        if item is None:
            return None, state

        update_id = item.get("id") or item.get("link") or item.get("title")
        if not update_id:
            return None, state

        next_state = CheckState(last_update_id=update_id)
        if update_id == state.last_update_id:
            return None, next_state

        return (
            {
                "update_id": update_id,
                "title": item.get("title"),
                "published_at": item.get("published_at"),
                "payload": item,
            },
            next_state,
        )


class YouTubeProvider(RssProvider):
    """Detect new YouTube uploads using the channel RSS feed."""

    def supports(self, source_type: str) -> bool:
        return source_type == "youtube_channel"

    def check_for_update(
        self, subscription: SourceSubscription, state: CheckState
    ) -> tuple[dict[str, Any] | None, CheckState]:
        channel_id = subscription.metadata.get("channel_id")
        if channel_id:
            feed_url = f"https://www.youtube.com/feeds/videos.xml?channel_id={quote_plus(channel_id)}"
        else:
            feed_url = subscription.source_url

        yt_subscription = SourceSubscription(
            source_id=subscription.source_id,
            source_type="rss",
            source_url=feed_url,
            target_services=subscription.target_services,
            metadata=subscription.metadata,
        )
        return super().check_for_update(yt_subscription, state)


YT_DLP_TIMEOUT_SECONDS = 120


class ToolFeedProvider(SourceProvider):
    """Detect new uploads on YouTube channels via yt-dlp extractor metadata.

    Only channel upload tabs are supported because they list the newest upload
    first, so the first entry is the latest one. Playlists can be in any order and
    are rejected. yt-dlp has no timeline extractor for X/Twitter accounts or
    Facebook pages, so those need a dedicated provider.
    """

    SUPPORTED_TYPES = {"youtube"}

    def supports(self, source_type: str) -> bool:
        return source_type in self.SUPPORTED_TYPES

    def check_for_update(
        self, subscription: SourceSubscription, state: CheckState
    ) -> tuple[dict[str, Any] | None, CheckState]:
        feed = self._load_feed(_channel_uploads_url(subscription.source_url))
        if "entries" not in feed:
            raise ValueError("source_url must resolve to a channel, not a single item")
        entries = feed.get("entries") or []
        latest = entries[0] if entries else None
        if latest is None:
            return None, state

        update_id = latest.get("id") or latest.get("url") or latest.get("webpage_url")
        if not update_id:
            return None, state

        next_state = CheckState(last_update_id=str(update_id))
        if next_state.last_update_id == state.last_update_id:
            return None, next_state

        channel = latest.get("channel") or feed.get("channel") or feed.get("uploader")
        uploader_id = latest.get("uploader_id") or feed.get("uploader_id")
        platform = subscription.source_type

        return (
            {
                "update_id": str(update_id),
                "title": latest.get("title") or feed.get("title"),
                "published_at": _parse_unix_timestamp(latest.get("timestamp"))
                or _parse_upload_date(latest.get("upload_date")),
                "payload": {
                    "tool": "yt-dlp",
                    # Flat-playlist entries are not fully extracted, so only the item's
                    # page URL is known; downstream ingestion resolves media from it.
                    "webpage_url": latest.get("webpage_url") or latest.get("url"),
                    "platform": platform,
                    "channel_name": channel,
                    "channel_id": latest.get("channel_id") or feed.get("channel_id"),
                    "uploader_id": uploader_id,
                    "raw_entry": latest,
                },
            },
            next_state,
        )

    def _load_feed(self, source_url: str) -> dict[str, Any]:
        result = subprocess.run(
            [
                "yt-dlp",
                "--dump-single-json",
                "--flat-playlist",
                "--playlist-end",
                "1",
                "--extractor-args",
                "youtubetab:approximate_date",
                "--",
                source_url,
            ],
            check=True,
            capture_output=True,
            text=True,
            timeout=YT_DLP_TIMEOUT_SECONDS,
        )
        return json.loads(result.stdout)


YOUTUBE_UPLOAD_TABS = {"videos", "shorts", "streams"}


def _channel_uploads_url(source_url: str) -> str:
    """Return the channel upload-tab URL for ``source_url`` or raise ValueError.

    Only upload tabs list the newest upload first. A root channel URL is
    normalized to its ``/videos`` tab because yt-dlp would otherwise return the
    channel's tabs as entries instead of its videos.
    """
    parsed = urlparse(source_url)
    if parsed.path.rstrip("/").endswith("/playlist") or "list" in parse_qs(parsed.query):
        raise ValueError(
            "youtube source_url must be a channel URL; playlists are not ordered by upload date"
        )

    host = (parsed.hostname or "").lower()
    segments = [segment for segment in parsed.path.split("/") if segment]
    if not segments or (host != "youtube.com" and not host.endswith(".youtube.com")):
        raise ValueError("youtube source_url must be a YouTube channel URL")

    if segments[0].startswith("@"):
        base_length = 1
    elif segments[0] in {"channel", "c", "user"} and len(segments) >= 2:
        base_length = 2
    else:
        raise ValueError("youtube source_url must be a YouTube channel URL")

    tab = segments[base_length] if len(segments) > base_length else "videos"
    if tab not in YOUTUBE_UPLOAD_TABS or len(segments) > base_length + 1:
        raise ValueError(
            "youtube source_url must point at a channel or one of its upload tabs "
            "(/videos, /shorts, /streams)"
        )

    path = "/" + "/".join([*segments[:base_length], tab])
    return f"https://{host}{path}"


def _extract_first_feed_item(root: ElementTree.Element) -> dict[str, Any] | None:
    namespaces = {
        "atom": "http://www.w3.org/2005/Atom",
        "yt": "http://www.youtube.com/xml/schemas/2015",
    }

    entry = root.find("channel/item")
    if entry is None:
        entry = root.find("atom:entry", namespaces)
    if entry is None:
        return None

    title = _text(entry, "title", namespaces)
    guid = _text(entry, "guid", namespaces) or _text(entry, "id", namespaces)
    link = _link(entry, namespaces)
    published = _text(entry, "pubDate", namespaces) or _text(entry, "published", namespaces)

    return {
        "id": guid,
        "title": title,
        "link": link,
        "published_at": _parse_http_date(published) if published else None,
        "youtube_video_id": _text(entry, "yt:videoId", namespaces),
        "youtube_channel_id": _text(entry, "yt:channelId", namespaces),
    }


def _text(node: ElementTree.Element, query: str, namespaces: dict[str, str]) -> str | None:
    match = node.find(query, namespaces)
    if match is None or match.text is None:
        return None
    return match.text.strip()


def _link(node: ElementTree.Element, namespaces: dict[str, str]) -> str | None:
    link_node = node.find("link")
    if link_node is not None:
        href = link_node.attrib.get("href")
        if href:
            return href
        if link_node.text:
            return link_node.text.strip()

    atom_link = node.find("atom:link", namespaces)
    if atom_link is not None:
        href = atom_link.attrib.get("href")
        if href:
            return href
    return None


def _parse_http_date(value: str | None) -> datetime | None:
    if not value:
        return None
    parsed = email.utils.parsedate_to_datetime(value)
    if parsed is None:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def _parse_unix_timestamp(value: Any) -> datetime | None:
    if value is None:
        return None
    try:
        return datetime.fromtimestamp(int(value), tz=timezone.utc)
    except (TypeError, ValueError, OSError):
        return None


def _parse_upload_date(value: Any) -> datetime | None:
    if not value:
        return None
    try:
        return datetime.strptime(str(value), "%Y%m%d").replace(tzinfo=timezone.utc)
    except ValueError:
        return None
