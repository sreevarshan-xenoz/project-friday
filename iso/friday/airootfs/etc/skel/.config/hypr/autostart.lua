-- Extra autostart processes.
-- o.launch_on_start("my-service")

-- FRIDAY: one-time first-login greeting (offers AI setup if no key yet).
o.exec_on_start("friday welcome")
