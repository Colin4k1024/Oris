from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, field
from typing import Any


REPLAY_SKIP = "skip"
REPLAY_SUGGEST = "suggest"
REPLAY_FORCE = "force"

_SPACE_PATTERN = re.compile(r"\s+")
_DIGIT_PATTERN = re.compile(r"\b\d+\b")
_PATH_PATTERN = re.compile(r"([A-Za-z]:)?[/\\][^\s:]+")


@dataclass
class EvolutionSignal:
    task_class: str
    error_type: str
    message: str
    fingerprint: str
    context: dict[str, Any] = field(default_factory=dict)


@dataclass
class EvolutionCandidate:
    gene_id: str
    confidence: float
    capsule_id: str = ""
    rationale: str = ""
    steps: list[str] = field(default_factory=list)
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class ReplayDecision:
    mode: str = REPLAY_SKIP
    candidate: EvolutionCandidate | None = None
    instructions: list[str] = field(default_factory=list)
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class ValidationResult:
    passed: bool
    evidence: str = ""
    metrics: dict[str, Any] = field(default_factory=dict)
    error: str = ""


@dataclass
class SolidifyInput:
    signal: EvolutionSignal
    solution: str
    validation: ValidationResult
    tags: list[str] = field(default_factory=list)
    steps: list[str] = field(default_factory=list)
    gene_id: str = ""
    name: str = ""


@dataclass
class ReplayPolicy:
    min_confidence: float = 0.0
    mode: str = REPLAY_SUGGEST
    limit: int = 5


@dataclass
class Status:
    genes_count: int = 0
    avg_confidence: float = 0.0
    reuse_rate: float = 0.0
    successful_uses: int = 0
    total_uses: int = 0


def detect_signal(error: Exception | str, task_class: str = "general", context: dict[str, Any] | None = None) -> EvolutionSignal:
    message = str(error)
    error_type = type(error).__name__.lower() if isinstance(error, Exception) else "error"
    normalized = normalize_message(message)
    return EvolutionSignal(
        task_class=task_class or "general",
        error_type=error_type or "error",
        message=message,
        fingerprint=fingerprint(task_class or "general", error_type or "error", normalized),
        context=context or {},
    )


def normalize_message(message: str) -> str:
    first_line = message.strip().splitlines()[0] if message.strip() else ""
    first_line = _PATH_PATTERN.sub("<path>", first_line)
    first_line = _DIGIT_PATTERN.sub("<num>", first_line)
    first_line = _SPACE_PATTERN.sub(" ", first_line)
    return first_line.strip().lower()


def fingerprint(*parts: str) -> str:
    joined = "|".join(parts)
    return hashlib.sha256(joined.encode()).hexdigest()[:24]

