export async function loadProfile(userId: string) {
  const res = await fetch(`/api/users/${userId}`);
  if (!res.ok) {
    throw new Error("profile load failed");
  }
  const data = await res.json();
  cache.set(userId, data);
  return data;
}
