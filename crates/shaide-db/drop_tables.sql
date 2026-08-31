_sqlx_migrations                refresh_tokens                
email_setting                   registration_token            
integrations                    repositories                  
invitations                     server_setting                
job_runs                        source_id_read_access_policies
ldap_credential                 thread_messages               
notifications                   threads                       
oauth_credential                user_completions              
page_sections                   user_events                   
pages                           user_group_memberships        
password_reset                  user_groups                   
provided_repositories           users                         
read_notifications              web_documents                 
DROP TABLE IF EXISTS _sqlx_migrations;
DROP TABLE IF EXISTS registration_token;
DROP TABLE IF EXISTS sqlite_sequence;
DROP TABLE IF EXISTS users;
DROP TABLE IF EXISTS invitations;
DROP TABLE IF EXISTS job_runs;
DROP TABLE IF EXISTS repositories;
DROP TABLE IF EXISTS server_setting;
DROP TABLE IF EXISTS email_setting;
DROP TABLE IF EXISTS oauth_credential;
DROP TABLE IF EXISTS user_completions;
DROP TABLE IF EXISTS user_events;
DROP TABLE IF EXISTS refresh_tokens;
DROP TABLE IF EXISTS password_reset;
DROP TABLE IF EXISTS integrations;
DROP TABLE IF EXISTS provided_repositories;
DROP TABLE IF EXISTS threads;
DROP TABLE IF EXISTS thread_messages;
DROP TABLE IF EXISTS web_documents;
DROP TABLE IF EXISTS user_groups;
DROP TABLE IF EXISTS user_group_memberships;
DROP TABLE IF EXISTS source_id_read_access_policies;
DROP TABLE IF EXISTS notifications;
DROP TABLE IF EXISTS read_notifications;
DROP TABLE IF EXISTS pages;
DROP TABLE IF EXISTS page_sections;
DROP TABLE IF EXISTS ldap_credential;
