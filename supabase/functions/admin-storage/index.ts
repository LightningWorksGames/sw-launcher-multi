/**
 * Edge Function: admin-storage
 *
 * Proxies admin storage operations for the launcher.
 * Verifies the user's SSO token against sso.lightningworks.io,
 * checks for admin/superadmin role, then performs the storage
 * operation using the service role key (never exposed to clients).
 *
 * Operations:
 *   POST /admin-storage  { action: "upload", filename, data (base64) }
 *   POST /admin-storage  { action: "delete", filename }
 *   POST /admin-storage  { action: "save-config", config: { greeting } }
 *   POST /admin-storage  { action: "save-order", order: ["file1.jpg", ...] }
 */

import { createClient } from "https://esm.sh/@supabase/supabase-js@2";

const SSO_VERIFY_URL = "https://sso.lightningworks.io/api/verify";
const BUCKET = "launcher-assets";

const CORS_HEADERS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "POST, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
};

function errorResponse(message: string, status: number): Response {
  return new Response(JSON.stringify({ error: message }), {
    status,
    headers: { ...CORS_HEADERS, "Content-Type": "application/json" },
  });
}

function okResponse(data: unknown = { ok: true }): Response {
  return new Response(JSON.stringify(data), {
    status: 200,
    headers: { ...CORS_HEADERS, "Content-Type": "application/json" },
  });
}

/** Verify SSO token and return user if admin. */
async function verifyAdmin(token: string): Promise<{ id: string; role: string; display_name: string }> {
  const res = await fetch(SSO_VERIFY_URL, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ token }),
  });

  if (!res.ok) {
    throw new Error("SSO verification failed");
  }

  const body = await res.json();
  if (!body.valid || !body.user) {
    throw new Error("Invalid or expired token");
  }

  const role = body.user.role || "user";
  if (role !== "admin" && role !== "superadmin") {
    throw new Error("Admin access required");
  }

  return body.user;
}

/** Get a Supabase client with service role privileges. */
function getServiceClient() {
  const url = Deno.env.get("SUPABASE_URL")!;
  const key = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!;
  return createClient(url, key);
}

Deno.serve(async (req: Request) => {
  // Handle CORS preflight
  if (req.method === "OPTIONS") {
    return new Response(null, { headers: CORS_HEADERS });
  }

  if (req.method !== "POST") {
    return errorResponse("Method not allowed", 405);
  }

  // Extract token from Authorization header
  const authHeader = req.headers.get("Authorization") || "";
  const token = authHeader.replace(/^Bearer\s+/i, "").trim();
  if (!token) {
    return errorResponse("Authorization header required", 401);
  }

  // Verify admin
  let user;
  try {
    user = await verifyAdmin(token);
  } catch (e) {
    return errorResponse(e.message, 403);
  }

  // Parse request body
  let body;
  try {
    body = await req.json();
  } catch {
    return errorResponse("Invalid JSON body", 400);
  }

  const { action } = body;
  const supabase = getServiceClient();

  try {
    switch (action) {
      case "upload": {
        const { filename, data: base64Data } = body;
        if (!filename || !base64Data) {
          return errorResponse("filename and data required", 400);
        }
        // Decode base64 to bytes
        const bytes = Uint8Array.from(atob(base64Data), (c) => c.charCodeAt(0));
        const contentType = filename.endsWith(".png")
          ? "image/png"
          : filename.endsWith(".webp")
            ? "image/webp"
            : "image/jpeg";

        const { error } = await supabase.storage
          .from(BUCKET)
          .upload(filename, bytes, { contentType, upsert: true });

        if (error) return errorResponse(`Upload failed: ${error.message}`, 500);

        const { data: urlData } = supabase.storage.from(BUCKET).getPublicUrl(filename);
        return okResponse({ url: urlData.publicUrl });
      }

      case "delete": {
        const { filename } = body;
        if (!filename) return errorResponse("filename required", 400);

        const { error } = await supabase.storage.from(BUCKET).remove([filename]);
        if (error) return errorResponse(`Delete failed: ${error.message}`, 500);
        return okResponse();
      }

      case "save-config": {
        const { config } = body;
        if (!config) return errorResponse("config required", 400);

        const bytes = new TextEncoder().encode(JSON.stringify(config));
        const { error } = await supabase.storage
          .from(BUCKET)
          .upload("launcher-config.json", bytes, {
            contentType: "application/json",
            upsert: true,
          });

        if (error) return errorResponse(`Save config failed: ${error.message}`, 500);
        return okResponse();
      }

      case "save-order": {
        const { order } = body;
        if (!Array.isArray(order)) return errorResponse("order array required", 400);

        const bytes = new TextEncoder().encode(JSON.stringify(order));
        const { error } = await supabase.storage
          .from(BUCKET)
          .upload("slide-order.json", bytes, {
            contentType: "application/json",
            upsert: true,
          });

        if (error) return errorResponse(`Save order failed: ${error.message}`, 500);
        return okResponse();
      }

      default:
        return errorResponse(`Unknown action: ${action}`, 400);
    }
  } catch (e) {
    return errorResponse(`Internal error: ${e.message}`, 500);
  }
});
