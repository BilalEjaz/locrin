export function load(id: string) {
  console.error("failed", id);
  console.warn("slow", id);
  logger.info("ok");
  return id;
}
