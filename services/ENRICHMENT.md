# Service Spec: Enrichment

## Responsibility

Perform NLP analysis on all extracted text from a media object — transcripts, OCR text, article bodies, tweet text. Produce a fully enriched document ready for indexing and profiling.

---

## Interface

### Input

Consumes from `queue: content.extracted`.

Reads the `ContentDocument` from MinIO: `minio://processed/{media_id}/content.json`.

This document is assembled by a **Content Assembler** step (part of this service) that merges `transcript.json`, `ocr_frames.json`, and any `body_text` from the original `MediaObject`.

### Output

An `EnrichedDocument` stored at `minio://processed/{media_id}/enriched.json`.

### Publishes To

`queue: content.enriched`

---

## Processing Steps

1. **Load** `ContentDocument` from MinIO
2. **Assemble full text** from transcript + OCR + body (in that priority order)
3. **Language detection** (confirm or detect via `langdetect`)
4. **Named Entity Recognition** — extract PERSON, ORG, GPE, PRODUCT, etc.
5. **Keyword extraction** — extract keyphrases
6. **Sentiment analysis** — document-level + per transcript segment
7. **Topic assignment** — match to topic taxonomy or cluster
8. **Embedding generation** — encode `full_text` to a dense vector
9. **Store** `enriched.json` in MinIO
10. **Publish** `content.enriched` event

---

## NLP Components

### Named Entity Recognition (NER)

**Library**: spaCy `en_core_web_trf` (transformer-based, most accurate)

```python
import spacy

nlp = spacy.load("en_core_web_trf")
doc = nlp(full_text[:512_000])  # spaCy has a character limit; chunk if needed

entities = []
for ent in doc.ents:
    if ent.label_ in {"PERSON", "ORG", "GPE", "PRODUCT", "EVENT", "NORP", "WORK_OF_ART", "LAW"}:
        entities.append({
            "text": ent.text,
            "label": ent.label_,
            "start_char": ent.start_char,
            "end_char": ent.end_char,
            "source": "transcript"  # or "ocr", "body", "title"
        })
```

**Note**: For very long documents, process in 50,000 character chunks with overlap to avoid missing entities at boundaries.

---

### Keyword Extraction

**Library**: KeyBERT

```python
from keybert import KeyBERT

kw_model = KeyBERT(model="all-MiniLM-L6-v2")
keywords = kw_model.extract_keywords(
    full_text,
    keyphrase_ngram_range=(1, 3),
    stop_words="english",
    use_maxsum=True,
    nr_candidates=20,
    top_n=15
)
# keywords = [("bitcoin halving", 0.87), ("proof of work", 0.81), ...]
```

---

### Sentiment Analysis

**Library**: HuggingFace Transformers

**Model**: `cardiffnlp/twitter-roberta-base-sentiment-latest`
(Handles informal/spoken language well — good for transcripts and tweets)

```python
from transformers import pipeline

sentiment_pipeline = pipeline(
    "sentiment-analysis",
    model="cardiffnlp/twitter-roberta-base-sentiment-latest",
    tokenizer="cardiffnlp/twitter-roberta-base-sentiment-latest"
)

# Document-level: truncate to 512 tokens
doc_sentiment = sentiment_pipeline(full_text[:1024])[0]

# Per-segment: run on each TranscriptSegment individually
segment_sentiments = []
for seg in transcript_segments:
    result = sentiment_pipeline(seg["text"][:512])[0]
    segment_sentiments.append({
        "segment_index": seg["segment_index"],
        "label": result["label"].lower(),
        "score": result["score"]
    })
```

**Label mapping**: The Cardiff model outputs `positive`, `negative`, `neutral`.

---

### Topic Assignment

**Phase 1 approach**: Keyword-based topic matching against a predefined taxonomy.

Define a taxonomy in `enrichment/topics.yaml`:
```yaml
topics:
  - label: "cryptocurrency"
    keywords: [bitcoin, ethereum, crypto, blockchain, defi, nft, web3, halving, altcoin]
  - label: "artificial_intelligence"
    keywords: [ai, machine learning, llm, gpt, neural network, deep learning, anthropic, openai]
  - label: "politics"
    keywords: [election, congress, senate, policy, government, democrat, republican]
  - label: "finance"
    keywords: [stock, investment, fed, interest rate, inflation, equity, hedge fund]
  # ... extend as needed
```

Match by counting keyword occurrences (case-insensitive) in `full_text`. Assign any topic with ≥2 keyword matches.

**Phase 5 upgrade**: Replace with BERTopic for data-driven topic discovery.

---

### Embedding Generation

**Library**: `sentence-transformers`

**Model**: `all-MiniLM-L6-v2` (384 dims, fast) or `all-mpnet-base-v2` (768 dims, higher quality)

```python
from sentence_transformers import SentenceTransformer

model = SentenceTransformer("all-mpnet-base-v2")

# For documents: encode a summary (title + first 500 chars of body)
text_to_embed = f"{title or ''}\n\n{full_text[:2000]}"
embedding = model.encode(text_to_embed).tolist()

# For segments: encode each segment text individually
for seg in transcript_segments:
    seg["embedding"] = model.encode(seg["text"]).tolist()
```

---

## Text Assembly Strategy

The `full_text` field used for NLP should be assembled in this order, with clear section markers:

```
[TITLE]
{title}

[DESCRIPTION]
{description}

[TRANSCRIPT]
{transcript segments joined by newline, formatted as "SPEAKER_00: text"}

[SCREEN TEXT]
{unique OCR text blocks, deduplicated, joined by newline}

[BODY]
{article/PDF body text}
```

Deduplication: if OCR text is already present in the transcript (auto-captioning), skip it.

---

## Language Handling

- Detect language from `full_text` using `langdetect` if not already set
- For non-English content, use the appropriate spaCy model (`es_core_news_trf`, `de_core_news_trf`, etc.)
- For sentiment, use a multilingual model: `lxyuan/distilbert-base-multilingual-cased-sentiments-student`
- For keywords/embeddings, use multilingual sentence transformers: `paraphrase-multilingual-MiniLM-L12-v2`

---

## Environment Variables

| Variable | Description |
|---|---|
| `REDIS_URL` | Queue connection |
| `MINIO_ENDPOINT` | MinIO host |
| `MINIO_ACCESS_KEY` | MinIO credentials |
| `MINIO_SECRET_KEY` | MinIO credentials |
| `SPACY_MODEL` | spaCy model name (default: `en_core_web_trf`) |
| `EMBEDDING_MODEL` | sentence-transformers model (default: `all-mpnet-base-v2`) |
| `SENTIMENT_MODEL` | HuggingFace model path |
| `TOPICS_CONFIG` | Path to `topics.yaml` |
| `DEVICE` | `cuda` or `cpu` |

---

## Notes on Chunking

NLP models have token limits. Apply this chunking strategy for long documents:

- **spaCy NER**: Process in 50,000 character chunks with 500 char overlap. Merge and deduplicate entities.
- **Sentiment**: Truncate to 512 tokens. For document-level, use the first 512 tokens (intro sets the tone).
- **Embeddings**: Encode max 384 tokens. Use the title + first 1500 chars as a representative summary.
- **Keywords**: Use the full text (KeyBERT handles long inputs by chunking internally).
