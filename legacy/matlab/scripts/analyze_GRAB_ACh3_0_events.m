% Runner Script for Analysis of GRABBER's data output
%
% Author: Douglas Albert
% Date: 2025-06-07 

clear; clc; close all;
import grabACHAnalysisTools.*;

% =================================================================
%                    USER CONFIGURATION
% =================================================================
config = struct();

% --- Path to Processed Data ---
config.processed_data_csv_path = '/path/to/your/data'; % <<< !!! UPDATE THIS PATH !!!
% --- Optional: Path to Parameters CSV from GRAB_ACh3_0_all_in_one.m (for robust fs loading) ---
config.preprocessing_params_csv_path = '/path/to/your/data'; % e.g., '/path/to/GRAB-ACh3.0_Parameters_....csv'; % <<< OPTIONAL UPDATE

% --- Event Definitions ---
% (Keep your existing 'events' struct definition method here, or see advanced note below)
events = struct();
%events(1).times_sec = [117];
%events(1).label     = 'Food_Introduced_FI';
%events(1).times_sec = [483];
%events(1).label     = 'Approach_Initiated_AI';
%events(2).times_sec = [383];
%events(2).label     = 'Feeding_Episode_End_EF';
%events(4).times_sec = [136, 191, 876, 902];
%events(4).label     = 'Sniff_Contact_Food_SF';
%events(5).times_sec = [1112];
%events(5).label     = 'Introduced_Social_Animal_IS';
%events(1).times_sec = [1393];
%events(1).label     = 'Social_Interaction_Start_SI';
%events(7).times_sec = [1];
%events(7).label     = 'Social_Interaction_End_SE';
%events(8).times_sec = [1377];
%events(8).label     = 'Social_Animal_Out_SO';
%events(1).times_sec = [312];
%events(1).label      = 'bite_food_BF';
events(1).times_sec  = [119]
events(1).label      = 'tail_suspension'
%events(1).times_sec   = [280]
%events(1).label       = 'airpuff_one'
%events(2).times_sec   = [309]
%events(2).label       = 'airpuff_two'
%events(3).times_sec   = [340]
%events(3).label       = 'airpuff_three'

config.events_to_analyze = events; % Store in the config struct

% --- Peri-Event Window Parameters ---
config.pre_event_sec = 4;
config.post_event_sec = 4;

% --- Trial-by-Trial Peri-Event Normalization Parameters ---
config.apply_trial_specific_normalization = true;
config.trial_normalization_method = 'zscore'; % 'zscore', 'dff_local', 'subtract_mean'
config.trial_norm_baseline_start_s = -10;
config.trial_norm_baseline_end_s   = -2;

% --- Analysis & Quantification Parameters ---
config.quant_params = struct();
config.quant_params.peak_window_start_sec = 0;
config.quant_params.peak_window_end_sec = 10;
config.quant_params.auc_window_start_sec = 0;
config.quant_params.auc_window_end_sec = 10;
config.quant_params.duration_threshold_signal = 1.5;
config.quant_params.duration_min_interval_sec = 0.3;

% --- Plotting Parameters ---
config.plotting = struct();
config.plotting.plot_individual_traces = true;
config.plotting.plot_average_trace = true;
config.plotting.plot_heatmap = true;
config.plotting.plot_summary_stats = true;

% --- Output Parameters ---
config.output_base_dir = './GRAB_ACh3_0_Flexible_Event_Analysis_Output';
config.save_plots_and_data = true;

% =================================================================
%                    INITIALIZATION & SETUP
% =================================================================
if config.save_plots_and_data && ~exist(config.output_base_dir, 'dir')
    mkdir(config.output_base_dir);
    fprintf('Created base output directory: %s\n', config.output_base_dir);
end

% Save the configuration used for this analysis run
if config.save_plots_and_data
    config_filename = fullfile(config.output_base_dir, ...
        sprintf('analysis_config_%s.mat', datestr(now,'yyyymmdd_HHMMSS')));
    save(config_filename, 'config');
    fprintf('Saved analysis configuration to: %s\n', config_filename);
end

% =================================================================
%                    DATA LOADING AND PREPARATION
% =================================================================
fprintf('Loading processed GRAB-ACh3.0 data from: %s\n', config.processed_data_csv_path);
if ~exist(config.processed_data_csv_path, 'file')
    error('GRABAnalysis:DataNotFound', 'Processed data CSV file not found: %s. Please update the path.', config.processed_data_csv_path);
end
data_table = readtable(config.processed_data_csv_path);

time_sec_col = 'Time_sec';
signal_col = 'DeltaF_over_F_Percent';

if ~ismember(time_sec_col, data_table.Properties.VariableNames) || ...
   ~ismember(signal_col, data_table.Properties.VariableNames)
    error('GRABAnalysis:MissingColumns', 'Required columns (%s, %s) not found in CSV. Check column names in %s.', time_sec_col, signal_col, config.processed_data_csv_path);
end

time_s = data_table.(time_sec_col);
signal_for_epochs = data_table.(signal_col);

% Handle NaNs (current strategy is fine, ensure it's documented or intended)
nan_idx = isnan(signal_for_epochs);
if any(nan_idx)
    fprintf('Warning: %d NaN values found in input signal. Replacing with zeros for epoch extraction. This may affect results if NaNs are extensive.\n', sum(nan_idx));
    signal_for_epochs(nan_idx) = 0; % Or consider interpolation if appropriate: fillmissing(signal_for_epochs, 'linear');
end

% Determine Sampling Rate (fs)
if ~isempty(config.preprocessing_params_csv_path) && exist(config.preprocessing_params_csv_path, 'file')
    try
        params_table = readtable(config.preprocessing_params_csv_path);
        fs_row = strcmp(params_table.Parameter, 'Sampling_Rate_Hz_Final');
        if any(fs_row)
            fs = params_table.Value(fs_row);
            fprintf('Loaded sampling rate from preprocessing params: %.2f Hz\n', fs);
        else
            warning('GRABAnalysis:FSParamNotFound', '"Sampling_Rate_Hz_Final" not found in params CSV. Estimating from time vector.');
            if length(time_s) < 2, error('GRABAnalysis:ShortData', 'Not enough data points to determine sampling rate from time vector.'); end
            fs = 1 / mean(diff(time_s));
            fprintf('Estimated sampling rate from time vector: %.2f Hz\n', fs);
        end
    catch ME
        warning('GRABAnalysis:FSLoadError', 'Could not load fs from params CSV: %s. Estimating from time vector.', ME.message);
        if length(time_s) < 2, error('GRABAnalysis:ShortData', 'Not enough data points to determine sampling rate from time vector.'); end
        fs = 1 / mean(diff(time_s));
        fprintf('Estimated sampling rate from time vector: %.2f Hz\n', fs);
    end
else
    if length(time_s) < 2, error('GRABAnalysis:ShortData', 'Not enough data points to determine sampling rate.'); end
    fs = 1 / mean(diff(time_s)); % Ensure time_s is sorted if loaded from unordered source
    fprintf('Estimated sampling rate from time vector: %.2f Hz\n', fs);
end
if isnan(fs) || fs <= 0, error('GRABAnalysis:InvalidFS', 'Failed to determine a valid sampling rate.'); end


% =================================================================
%              PERFORM ANALYSIS FOR EACH DEFINED EVENT TYPE
% =================================================================

if isempty(config.events_to_analyze) || ~isfield(config.events_to_analyze, 'times_sec') || isempty(config.events_to_analyze(1).times_sec)
    warning('GRABAnalysis:NoEvents', 'No events defined or event definitions are empty. Skipping peri-event analysis.');
    return;
end

all_event_analysis_results = struct(); % To store results for all event types

for event_type_idx = 1:length(config.events_to_analyze)
    current_event_config = config.events_to_analyze(event_type_idx); % Get current event
    
    if ~isfield(current_event_config, 'times_sec') || ~isfield(current_event_config, 'label') || isempty(current_event_config.times_sec)
        fprintf('Skipping event definition %d as it is incomplete or has no timestamps.\n', event_type_idx);
        continue;
    end

    current_event_times_s = current_event_config.times_sec;
    current_event_label = current_event_config.label;
    
    fprintf('\n--- Processing Event Type: %s (%d instances) ---\n', current_event_label, length(current_event_times_s));

    current_output_dir_event = fullfile(config.output_base_dir, current_event_label);
    if config.save_plots_and_data && ~exist(current_output_dir_event, 'dir'), mkdir(current_output_dir_event); end

    % --- Peri-Event Epoch Extraction ---
    fprintf('Extracting peri-event epochs for "%s"...\n', current_event_label);
    % NOTE: Update to use package syntax
    [epochs_extracted, epoch_time_s_relative, valid_event_indices] = ...
        grabACHAnalysisTools.extract_peri_event_epochs(time_s, signal_for_epochs, current_event_times_s, ...
                                                       config.pre_event_sec, config.post_event_sec, fs);

    if isempty(epochs_extracted)
        fprintf('No valid epochs extracted for "%s". Skipping further analysis for this event type.\n', current_event_label);
        all_event_analysis_results.(current_event_label).status = 'No valid epochs';
        continue;
    end
    fprintf('%d valid epochs extracted for "%s".\n', size(epochs_extracted, 2), current_event_label);
    all_event_analysis_results.(current_event_label).epochs_extracted = epochs_extracted;
    all_event_analysis_results.(current_event_label).epoch_time_s_relative = epoch_time_s_relative;
    all_event_analysis_results.(current_event_label).valid_event_indices_original = valid_event_indices; % Store original indices
    all_event_analysis_results.(current_event_label).valid_event_times_s = current_event_times_s(valid_event_indices);


    % --- Optional: Trial-by-Trial Peri-Event Normalization ---
    epochs_for_analysis = epochs_extracted; % Start with the extracted epochs
    current_normalization_string = "Globally Processed Signal"; % Default
    trial_baselines_info_table = []; % Initialize

    if config.apply_trial_specific_normalization
       fprintf('Applying trial-by-trial normalization ("%s") using baseline window [%.1fs to %.1fs]...\n', ...
               config.trial_normalization_method, config.trial_norm_baseline_start_s, config.trial_norm_baseline_end_s);
       
       % NOTE: Update to use package syntax
       [epochs_for_analysis, trial_baselines_info_struct] = ...
           grabACHAnalysisTools.normalize_epochs_trial_by_trial(epochs_extracted, epoch_time_s_relative, ...
                                                                config.trial_norm_baseline_start_s, ...
                                                                config.trial_norm_baseline_end_s, ...
                                                                config.trial_normalization_method);
       current_normalization_string = sprintf("Trial-Norm: %s (Baseline: %.1fs to %.1fs)", ...
            config.trial_normalization_method, config.trial_norm_baseline_start_s, config.trial_norm_baseline_end_s);
        all_event_analysis_results.(current_event_label).epochs_trial_normalized = epochs_for_analysis;
        all_event_analysis_results.(current_event_label).trial_baselines_info = trial_baselines_info_struct;


       if config.save_plots_and_data && ~isempty(trial_baselines_info_struct)
            original_event_times_for_baseline = current_event_times_s(valid_event_indices);
            
            temp_table = struct2table(trial_baselines_info_struct);
            trial_baselines_info_table = table(valid_event_indices(:), original_event_times_for_baseline(:), ...
                                        temp_table.mean, temp_table.std, ...
                                        'VariableNames', {'Epoch_Index_Within_Valid', 'Original_Event_Time_s', 'Baseline_Mean', 'Baseline_Std'});

            baseline_info_filename = fullfile(current_output_dir_event, sprintf('%s_trial_baseline_info.csv', current_event_label));
            writetable(trial_baselines_info_table, baseline_info_filename);
            fprintf('Saved trial baseline info to: %s\n', baseline_info_filename);
       end
    else
        fprintf('Skipping trial-by-trial normalization. Using globally processed signal for epochs.\n');
        all_event_analysis_results.(current_event_label).epochs_trial_normalized = []; % Indicate not performed
        all_event_analysis_results.(current_event_label).trial_baselines_info = [];
    end
    all_event_analysis_results.(current_event_label).normalization_method_string = current_normalization_string;


    % --- Response Quantification ---
    fprintf('Quantifying responses for "%s"...\n', current_event_label);
    % Pass the relevant sub-struct of config
    % NOTE: Update to use package syntax
    event_metrics_struct = quantify_event_responses(epochs_for_analysis, epoch_time_s_relative, fs, config.quant_params);
    all_event_analysis_results.(current_event_label).event_metrics = event_metrics_struct;
    
    metrics_table = []; % Initialize
    if ~isempty(event_metrics_struct)
        original_event_times_for_metrics = current_event_times_s(valid_event_indices);
        metrics_table = table(valid_event_indices(:), original_event_times_for_metrics(:), ...
                              'VariableNames', {'Epoch_Index_Within_Valid', 'Original_Event_Time_s'});
        metrics_table = [metrics_table, struct2table(event_metrics_struct)];

        if config.save_plots_and_data
            metrics_filename = fullfile(current_output_dir_event, sprintf('%s_event_metrics.csv', current_event_label));
            writetable(metrics_table, metrics_filename);
            fprintf('Saved quantified event metrics to: %s\n', metrics_filename);
        end
    else
        fprintf('No quantifiable metrics generated for "%s" or no valid events for metrics table.\n', current_event_label);
    end
    
    % --- Visualization ---
    fprintf('Generating plots for "%s"...\n', current_event_label);
    plot_params_event = struct(); % Create a plot_params struct for this event
    plot_params_event.pre_event_sec = config.pre_event_sec; % Pass relevant params
    plot_params_event.post_event_sec = config.post_event_sec;
    plot_params_event.event_label = current_event_label;
    plot_params_event.output_dir = current_output_dir_event;
    plot_params_event.save_plots = config.save_plots_and_data;
    plot_params_event.fs = fs;
    plot_params_event.normalization_method_string = current_normalization_string;

    if config.plotting.plot_individual_traces && ~isempty(epochs_for_analysis)
        grabACHAnalysisTools.plot_individual_event_traces(epochs_for_analysis, epoch_time_s_relative, plot_params_event);
    end
    if config.plotting.plot_average_trace && ~isempty(epochs_for_analysis)
        grabACHAnalysisTools.plot_average_event_trace(epochs_for_analysis, epoch_time_s_relative, plot_params_event);
    end
    if config.plotting.plot_heatmap && ~isempty(epochs_for_analysis)
        grabACHAnalysisTools.plot_event_heatmap(epochs_for_analysis, epoch_time_s_relative, plot_params_event);
    end
    if config.plotting.plot_summary_stats && ~isempty(event_metrics_struct)
        grabACHAnalysisTools.plot_summary_statistics(event_metrics_struct, plot_params_event);
    end
    
    fprintf('Peri-event analysis for "%s" completed.\n', current_event_label);
end

% Save all results in one .mat file
if config.save_plots_and_data
    results_filename = fullfile(config.output_base_dir, ...
        sprintf('all_event_analysis_results_%s.mat', datestr(now,'yyyymmdd_HHMMSS')));
    save(results_filename, 'all_event_analysis_results', 'config'); % Save config too
    fprintf('Saved all event analysis results to: %s\n', results_filename);
end

fprintf('\nAll GRAB-ACh3.0 event processing finished.\n');