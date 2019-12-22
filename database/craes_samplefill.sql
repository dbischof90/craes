
-- Sample values to fill into CRAES DB.

-- 1) Set up two assets
INSERT INTO asset_info 
VALUES
    (1, 'A'),
    (2, 'B')
;

-- 2) Set up test user
INSERT INTO user_info
VALUES
    ('test_user', '$2b$12$kOB1jqjNELk8h.Kp1cR.ueE7jD.bbeiBex3KMzsaS9xrbyLWs.wgK')
;

