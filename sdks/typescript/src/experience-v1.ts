/** Generated-shape models for spec/experience/experience-bundle-v1.schema.json. */
export type ExperienceScope = "local" | "project" | "tenant" | "team" | "network";
export type LifecycleState = "candidate" | "stable" | "deprecated" | "quarantined" | "revoked";
export type OutcomeStatus = "succeeded" | "failed" | "safety_failed" | "inconclusive";

export interface ExperienceBundleV1 { schema_version:"oris.experience.bundle/v1"; gene:GeneV1; capsules:CapsuleV1[]; usage_receipts:UsageReceiptV1[] }
export interface GeneV1 { id:string; version:number; name:string; description:string; scope:ExperienceScope; task_category:string; applicability:Applicability; steps:ProcedureStep[]; tool_requirements:ToolRequirement[]; safety:SafetyConstraints; validation:ValidationContract; provenance:Provenance; lifecycle:LifecycleState; created_at:string; updated_at:string; metadata:Record<string,unknown> }
export interface Applicability { required_signals:string[]; excluded_signals:string[]; environments:EnvironmentConstraint[]; project_ids:string[]; tenant_ids:string[]; do_not_use_when:string[] }
export interface EnvironmentConstraint { key:string; operator:"equals"|"not_equals"|"contains"|"semver"|"exists"; value:unknown }
export interface ProcedureStep { id:string; instruction:string; tool:string|null; requires_approval:boolean; expected_output:string|null }
export interface ToolRequirement { name:string; minimum_version:string|null; required_permissions:string[] }
export interface SafetyConstraints { suggestion_only:boolean; forbidden_operations:string[]; required_approvals:string[]; secret_handling:"redact"|"reject"|"allow_references_only" }
export interface ValidationContract { checks:ValidationCheck[]; success_condition:"all"|"any"|"human_approved" }
export interface ValidationCheck { id:string; command_or_assertion:string; evidence_kind:"test"|"command"|"diff"|"artifact"|"human_approval"; timeout_seconds:number|null }
export interface Provenance { source_agent:string; source_run_id:string; trace_refs:string[]; extractor_version:string|null; verified_successes:number; verified_failures:number; distinct_task_contexts:number }
export interface CapsuleV1 { id:string; gene_id:string; gene_version:number; environment_fingerprint:string; task_context_hash:string; execution_evidence_hash:string; validation:Record<string,unknown>; artifact_refs:string[]; redaction:"redacted"|"verified_clean"|"contains_references_only"|"rejected"; created_at:string }
export interface UsageReceiptV1 { id:string; gene_id:string; gene_version:number; agent_id:string; run_id:string; task_context_hash:string; adoption:"adopted"|"partially_adopted"|"rejected"|"not_applicable"; applied_step_ids:string[]; outcome:OutcomeStatus; failure_reason?:string|null; test_evidence_refs:string[]; cost?:Record<string,unknown>|null; created_at:string }

export function validateExperienceBundleV1(bundle:ExperienceBundleV1):void {
  if(bundle.schema_version!=="oris.experience.bundle/v1") throw new Error("unsupported experience schema");
  if(!bundle.gene.steps.length||!bundle.gene.validation.checks.length) throw new Error("Gene requires steps and validation checks");
  for(const receipt of bundle.usage_receipts) if(receipt.adoption==="adopted"&&receipt.outcome==="succeeded"&&!receipt.test_evidence_refs.length) throw new Error("successful adoption requires evidence");
}
