/** Architecture draft only. No production IPC is implemented by this file. */
export type EngineId = 'dsh' | 'pi';
export type PlatformId = 'windows-x86_64' | 'windows-aarch64' | 'macos-aarch64' | 'macos-x86_64' | 'linux-x86_64';
export type Verification = 'unverified' | 'verified' | 'blocked';
export type InstallState = 'missing' | 'resolving' | 'downloading' | 'verifying' | 'staging' | 'ready' | 'failed';
export type RunState = 'discovering' | 'stopped' | 'starting' | 'healthy' | 'degraded' | 'stopping' | 'failed';
export type ExtensionKind = 'dsh-plugin' | 'pi-extension' | 'skill' | 'mcp-server' | 'web-ui';
export interface ArtifactRef {
  source: string; exactVersion: string; sha256: string; platform: PlatformId;
  license: string; dependencyLockSha256?: string;
}
export interface ExtensionLock {
  id: string; kind: ExtensionKind; enabled: boolean;
  artifact: ArtifactRef; adapterMappingVersion: string;
}
export interface ValidatedProfile {
  schemaVersion: 1; id: string; engine: EngineId; platform: PlatformId;
  runtime: ArtifactRef; engineArtifact: ArtifactRef;
  web: { kind: 'bundled' } | { kind: 'separate'; artifact: ArtifactRef };
  extensions: ExtensionLock[]; adapterVersion: string;
  verification: Verification; evidence: { testedAt: string; reportId: string }[];
}
export interface Instance {
  id: string; name: string; engine: EngineId; profileId: string;
  workspaceRef: string; activeRevision: string; revision: number;
  installState: InstallState; runState: RunState; lastError?: DomainError;
}
export interface PortableRecipe {
  schemaVersion: 1; format: 'perch.recipe'; id: string; name: string;
  engine: EngineId; validatedProfileId: string | null;
  extensionIds: string[]; status: 'draft' | 'ready';
  // No executable, shell script, secret or local absolute path fields.
}
export interface Snapshot {
  id: string; instanceId: string; createdAt: string; revision: string;
  lockSha256: string; configArchiveSha256: string;
  artifactRefs: string[]; excludes: ('secrets' | 'workspace' | 'sessions')[];
}
export interface DomainError {
  code: 'PROFILE_UNVERIFIED' | 'INCOMPATIBLE_EXTENSION' | 'ARTIFACT_MISSING'
    | 'INTEGRITY_FAILED' | 'PORT_CONFLICT' | 'START_TIMEOUT' | 'PROCESS_EXITED'
    | 'WORKSPACE_MISSING' | 'OPERATION_CONFLICT' | 'CONFIG_REQUIRED'
    | 'PERMISSION_DENIED' | 'CANCELLED' | 'INTERNAL';
  message: string; retryable: boolean; operationId?: string; diagnosticId?: string;
}
export interface Operation {
  id: string; instanceId?: string; kind: 'install' | 'start' | 'stop' | 'update' | 'restore';
  phase: string; state: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
  progress: number | null; cancellable: boolean; error?: DomainError;
}
export type Result<T> = { ok: true; value: T } | { ok: false; error: DomainError };
export interface CommandContext { requestId: string; idempotencyKey: string; expectedRevision?: number }
export interface DesktopGateway {
  listInstances(): Promise<Result<Instance[]>>;
  createInstance(input: { name: string; profileId: string; workspaceRef: string }, ctx: CommandContext): Promise<Result<Instance>>;
  installInstance(instanceId: string, ctx: CommandContext): Promise<Result<Operation>>;
  startInstance(instanceId: string, ctx: CommandContext): Promise<Result<Operation>>;
  stopInstance(instanceId: string, ctx: CommandContext): Promise<Result<Operation>>;
  openWorkspace(instanceId: string): Promise<Result<void>>;
  validateRecipe(recipe: PortableRecipe): Promise<Result<{ installable: boolean; reasons: string[] }>>;
  snapshotInstance(instanceId: string, ctx: CommandContext): Promise<Result<Snapshot>>;
  restoreSnapshot(snapshotId: string, ctx: CommandContext): Promise<Result<Operation>>;
  cancelOperation(operationId: string): Promise<Result<void>>;
}
export type DesktopEvent =
  | { type: 'instance.changed'; sequence: number; instance: Instance }
  | { type: 'operation.changed'; sequence: number; operation: Operation }
  | { type: 'logs.appended'; sequence: number; instanceId: string; cursor: string; lines: string[] };
