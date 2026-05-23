const { createClient } = require("@supabase/supabase-js");

const supabaseUrl = process.env.SUPABASE_URL;
const supabaseKey = process.env.SUPABASE_SERVICE_KEY;

let supabase = null;
if (supabaseUrl && supabaseKey) {
  supabase = createClient(supabaseUrl, supabaseKey);
}

async function validateSubscription(req, res, next) {
  const { licenseKey, machineId } = req.body;

  if (!licenseKey || typeof licenseKey !== "string") {
    return res.status(403).json({ ok: false, error: "Missing or invalid license key" });
  }

  if (!supabase) {
    console.warn("[VALIDATE] Supabase not configured — allowing request by default");
    req.machineId = machineId || null;
    return next();
  }

  try {
    const { data, error } = await supabase
      .from("licenses")
      .select("id, machine_id")
      .eq("key", licenseKey)
      .eq("status", "active")
      .maybeSingle();

    if (error) {
      console.error("[VALIDATE] Supabase query error:", error.message);
      return res.status(403).json({ ok: false, error: "License validation failed" });
    }

    if (!data) {
      return res.status(403).json({ ok: false, error: "Invalid or inactive license key" });
    }

    req.machineId = data.machine_id || machineId || null;
    next();
  } catch (err) {
    console.error("[VALIDATE] Unexpected error:", err.message);
    return res.status(500).json({ ok: false, error: "Internal server error" });
  }
}

module.exports = { validateSubscription };
