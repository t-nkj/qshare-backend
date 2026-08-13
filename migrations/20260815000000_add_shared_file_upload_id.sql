ALTER TABLE `shared_files`
    ADD COLUMN `upload_id` CHAR(36) NULL AFTER `id`;

UPDATE `shared_files`
SET `upload_id` = `id`
WHERE `upload_id` IS NULL;

ALTER TABLE `shared_files`
    MODIFY COLUMN `upload_id` CHAR(36) NOT NULL,
    ADD INDEX `idx_shared_files_user_upload` (`user_id`, `upload_id`, `created_at`, `id`);
