import type { InstalledSkill } from "@/lib/api/skills";

/**
 * 合并本次导入/安装结果到已安装缓存，按 id 去重。
 *
 * 同一技能重复出现时以新记录为准（imported 内部重复时以最后一条为准），
 * 避免 mutation 被重复触发、或同一 ZIP 内出现同名目录时
 * 在 installed 列表中看到重复条目（重复的 React key）。
 * imported 为空时返回原引用，让 React Query 跳过无谓的订阅者通知。
 */
export function mergeImportedSkills(
  existing: InstalledSkill[] | undefined,
  imported: InstalledSkill[],
): InstalledSkill[] {
  const latestById = new Map<string, InstalledSkill>();
  for (const skill of imported) {
    latestById.delete(skill.id);
    latestById.set(skill.id, skill);
  }
  const deduped =
    latestById.size === imported.length
      ? imported
      : Array.from(latestById.values());
  if (!existing) return deduped;
  if (deduped.length === 0) return existing;
  const preserved = existing.filter((s) => !latestById.has(s.id));
  return [...preserved, ...deduped];
}
