export function load(id: string) {
  console.log("kept on purpose"); // locrin:allow
  const console = { log: (x: string) => x };
  console.log("shadowed local console");
  return id;
}
