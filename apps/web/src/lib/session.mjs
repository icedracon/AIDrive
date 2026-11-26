export async function requestSignOut(request = fetch) {
  try {
    const response = await request("/api/auth/logout", { method: "POST" });
    return response.status === 204;
  } catch {
    return false;
  }
}
