---
title: "RFC: "
labels:
  - rfc
body:
  - type: markdown
    attributes:
      value: |
        Use this template for Major Architecture Change proposals that require Architecture Review Board review.
  - type: textarea
    id: motivation
    attributes:
      label: Motivation
      description: What problem or opportunity requires this RFC?
      placeholder: Describe the operational, product, or ecosystem need.
    validations:
      required: true
  - type: textarea
    id: proposed-change
    attributes:
      label: Proposed Change
      description: What should change and what parts of Oris does it affect?
      placeholder: Describe the architecture, APIs, protocols, or workflows being proposed.
    validations:
      required: true
  - type: textarea
    id: alternatives
    attributes:
      label: Alternatives Considered
      description: What other approaches were evaluated and why were they not chosen?
      placeholder: List viable alternatives, tradeoffs, and rejected directions.
    validations:
      required: true
  - type: textarea
    id: backward-compatibility
    attributes:
      label: Backward Compatibility
      description: What compatibility risks or contract changes should reviewers evaluate?
      placeholder: Note wire compatibility, API stability, rollout risks, and any breaking behavior.
    validations:
      required: true
  - type: textarea
    id: migration-plan
    attributes:
      label: Migration Plan
      description: How will users, operators, or downstream integrations adopt this safely?
      placeholder: Describe rollout steps, feature flags, deprecation windows, and observability needs.
    validations:
      required: true