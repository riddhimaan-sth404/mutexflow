const { createClient } = require("@supabase/supabase-js");

const supabaseUrl = process.env.SUPABASE_URL;
const supabaseKey = process.env.SUPABASE_SERVICE_KEY;

let supabase = null;
if (supabaseUrl && supabaseKey) {
  supabase = createClient(supabaseUrl, supabaseKey);
}

async function validateSubscription(licenseKey) {
  if (!licenseKey || typeof licenseKey !== "string") {
    return false;
  }

  if (!supabase) {
    console.warn("[VALIDATE] Supabase not configured — allowing request by default");
    return true;
  }

  try {
    const { data, error } = await supabase
      .from("licenses")
      .select("id")
      .eq("key", licenseKey)
      .eq("status", "active")
      .maybeSingle();

    if (error) {
      console.error("[VALIDATE] Supabase query error:", error.message);
      return false;
    }

    return data !== null;
  } catch (err) {
    console.error("[VALIDATE] Unexpected error:", err.message);
    return false;
  }
}

module.exports = { validateSubscription };
