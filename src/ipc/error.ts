// Error kinds from crates/core/src/error.rs.
import type { AppError } from "./bindings/AppError";
import type { ErrorKind } from "./bindings/ErrorKind";

export const ERROR_KINDS: readonly ErrorKind[] = [
  "io",
  "parse",
  "not_found",
  "invalid_input",
  "permission",
  "target_conflict",
  "cancelled",
  "network",
  "database",
  "internal",
];

const validKinds: ReadonlySet<string> = new Set(ERROR_KINDS);

export function isAppError(error: unknown): error is AppError {
  return (
    typeof error === "object" &&
    error !== null &&
    typeof (error as AppError).message === "string" &&
    validKinds.has((error as AppError).kind)
  );
}

export function getErrorMessage(error: unknown, fallback: string): string {
  if (isAppError(error)) return error.message;
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === "string" && error) return error;
  return fallback;
}

export function getErrorKind(error: unknown): ErrorKind | undefined {
  return isAppError(error) ? error.kind : undefined;
}
