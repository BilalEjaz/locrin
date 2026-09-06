export async function loadUser(id: string) {
  const res = await fetch(`/api/users/${id}`);
  if (!res.ok) {
    throw new Error("user load failed");
  }
  const data = await res.json();
  cache.set(id, data);
  return data;
}

export function unrelated(list: number[]) {
  let sum = 0;
  for (const n of list) {
    if (n > 0) {
      sum += n;
    }
  }
  return sum / Math.max(list.length, 1);
}

export const tinyHelper = (n: number) => n + 1;
