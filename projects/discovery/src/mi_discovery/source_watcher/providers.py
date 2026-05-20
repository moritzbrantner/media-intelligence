"""Source providers that know how to detect updates for different source types."""

from __future__ import annotations

from abc import ABC, abstractmethod
from datetime import datetime, timezone
import email.utils
from typing import Any
from urllib.parse import quote_plus
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

