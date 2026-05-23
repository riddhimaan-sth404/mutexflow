const { supabase } = require("../config/db");

async function validateSubscription(req, res, next) {
  const { licenseKey, machineId } = req.body;

  if (!licenseKey || typeof licenseKey !== "string") {
    return res.status(403).json({ ok: false, error: "That doesn't look like a valid license key" });
  }

  if (!supabase) {
    console.warn("[VALIDATE] Supabase isn't configured — letting this one through");
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
      console.error("[VALIDATE] Couldn't check the license:", error.message);
      return res.status(403).json({ ok: false, error: "Couldn't verify your license right now" });
    }

    if (!data) {
      return res.status(403).json({ ok: false, error: "That license key doesn't look active" });
    }

    req.machineId = data.machine_id || machineId || null;
    next();
  } catch (err) {
    console.error("[VALIDATE] Something unexpected happened:", err.message);
    return res.status(500).json({ ok: false, error: "Internal server error" });
  }
}

module.exports = { validateSubscription };
