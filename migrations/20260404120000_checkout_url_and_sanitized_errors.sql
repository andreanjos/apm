ALTER TABLE orders
ADD COLUMN IF NOT EXISTS checkout_url TEXT;

UPDATE orders
SET checkout_url = CONCAT('https://checkout.stripe.com/pay/', checkout_session_id)
WHERE checkout_url IS NULL;

ALTER TABLE orders
ALTER COLUMN checkout_url SET NOT NULL;
