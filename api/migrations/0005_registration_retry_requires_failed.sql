CREATE TRIGGER registrations_retry_requires_failed
BEFORE UPDATE OF status ON registrations
WHEN NEW.status = 'registered'
 AND OLD.status <> 'failed'
BEGIN
    SELECT RAISE(ABORT, 'registration_retry_requires_failed');
END;
