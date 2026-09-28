CREATE TRIGGER IF NOT EXISTS credentials_require_active_reservation_insert
BEFORE INSERT ON credentials
WHEN NEW.status = 'issued'
 AND NEW.reservation_id IS NOT NULL
 AND NEW.reservation_id <> ''
 AND NOT EXISTS (
    SELECT 1 FROM reservations
    WHERE id = NEW.reservation_id
      AND owner_id = NEW.owner_id
      AND status = 'active'
 )
BEGIN
    SELECT RAISE(ABORT, 'credential_requires_active_reservation');
END;

CREATE TRIGGER IF NOT EXISTS credentials_require_active_reservation_update
BEFORE UPDATE OF status, reservation_id, owner_id ON credentials
WHEN NEW.status = 'issued'
 AND NEW.reservation_id IS NOT NULL
 AND NEW.reservation_id <> ''
 AND NOT EXISTS (
    SELECT 1 FROM reservations
    WHERE id = NEW.reservation_id
      AND owner_id = NEW.owner_id
      AND status = 'active'
 )
BEGIN
    SELECT RAISE(ABORT, 'credential_requires_active_reservation');
END;
