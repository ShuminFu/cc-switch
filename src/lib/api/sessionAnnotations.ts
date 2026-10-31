import { invoke } from "./invoke";

/** User annotation on a session (SESS-18): pin, archive and tags. */
export interface SessionAnnotation {
  pinned: boolean;
  archived: boolean;
  tags: string[];
}

/** Annotations keyed by session key (`provider:session:source`). */
export type SessionAnnotations = Record<string, SessionAnnotation>;

export const EMPTY_SESSION_ANNOTATION: SessionAnnotation = {
  pinned: false,
  archived: false,
  tags: [],
};

export const sessionAnnotationsApi = {
  list(): Promise<SessionAnnotations> {
    return invoke<SessionAnnotations>("get_session_annotations");
  },

  /** Replace one session's annotation; returns the stored (normalized) value. */
  set(
    sessionKey: string,
    annotation: SessionAnnotation,
  ): Promise<SessionAnnotation> {
    return invoke<SessionAnnotation>("set_session_annotation", {
      sessionKey,
      annotation,
    });
  },
};
