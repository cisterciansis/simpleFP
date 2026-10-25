function fig_handle = plot_average_event_trace(epochs_signal, epoch_time_s_relative, plot_params)
% PLOT_AVERAGE_EVENT_TRACE Plots average trace with SEM/STD shading.
% Args: see original file
% Returns: see original file

    arguments
        epochs_signal (:,:) double {mustBeNumeric, mustBeReal}
        epoch_time_s_relative (:,1) double {mustBeNumeric, mustBeReal}
        plot_params (1,1) struct 
    end

    expected_plot_param_fields = {'event_label', 'normalization_method_string', 'output_dir', 'save_plots'};
    grabACHAnalysisTools.mustContainFields(plot_params, expected_plot_param_fields);

    fig_handle = [];
    if isempty(epochs_signal), disp('Info: No epochs for plot_average_event_trace.'); return; end
    if size(epochs_signal, 1) ~= length(epoch_time_s_relative), error('grabACHAnalysisTools:plot_average_event_trace:DimensionMismatch', 'Epoch signal rows must match time vector length.'); end
    num_trials = size(epochs_signal, 2);
    if num_trials == 0, disp('Info: epochs_signal has zero trials. Skipping plot.'); return; end
    
    mean_trace = mean(epochs_signal, 2, 'omitnan');
    error_metric_type = upper(string(grabACHAnalysisTools.get_plot_param(plot_params, 'error_metric', "SEM")));
    if ~ismember(error_metric_type, ["SEM", "STD"]), warning('grabACHAnalysisTools:plot_average_event_trace:InvalidErrorMetric', 'Invalid error_metric. Defaulting to SEM.'); error_metric_type = "SEM"; end
    error_values = zeros(size(mean_trace), 'like', mean_trace);
    if num_trials > 1, std_trace = std(epochs_signal, 0, 2, 'omitnan'); if error_metric_type == "SEM", error_values = std_trace / sqrt(max(1,num_trials)); else, error_values = std_trace; end; else error_metric_type = "None (1 trial)"; end % max(1,num_trials) for safety if num_trials became 0 unexpectedly
    mean_trace(isnan(mean_trace)) = 0; error_values(isnan(error_values)) = 0;

    y_label_str = plot_params.normalization_method_string;
    figure_name_str = sprintf('%s: Average Trace +/- %s (%s)', strrep(plot_params.event_label, '_', ' '), error_metric_type, plot_params.normalization_method_string);
    
    fig_handle = figure('Name', figure_name_str, 'NumberTitle', 'off', 'Visible', 'on');
    fig_pos = grabACHAnalysisTools.get_plot_param(plot_params, 'figure_position', [150, 150, 800, 600]);
    set(fig_handle, 'Position', fig_pos);
    
    hold on;
    mean_color = grabACHAnalysisTools.get_plot_param(plot_params, 'mean_trace_color', [0.1 0.3 0.6]);
    shade_color = grabACHAnalysisTools.get_plot_param(plot_params, 'shade_color', [0.2 0.5 0.8]);
    shade_alpha = grabACHAnalysisTools.get_plot_param(plot_params, 'shade_alpha', 0.3);
    line_width_val = grabACHAnalysisTools.get_plot_param(plot_params, 'line_width', 2);

    if num_trials > 1 && any(error_values > 0), fill([epoch_time_s_relative; flipud(epoch_time_s_relative)], [mean_trace - error_values; flipud(mean_trace + error_values)], shade_color, 'FaceAlpha', shade_alpha, 'EdgeColor', 'none'); end
    plot(epoch_time_s_relative, mean_trace, 'Color', mean_color, 'LineWidth', line_width_val);
    xline(0, 'k--', 'LineWidth', 1.5, 'Label', 'Event Onset', 'LabelHorizontalAlignment', 'center', 'LabelVerticalAlignment', 'bottom');
    yline(0, 'r:', 'LineWidth', 1.0, 'Alpha', 0.7); 
    hold off;
    
    xlabel(sprintf('Time from %s Onset (s)', strrep(plot_params.event_label, '_', ' '))); ylabel(y_label_str); title(figure_name_str, 'Interpreter', 'none');
    x_lims = grabACHAnalysisTools.get_plot_param(plot_params, 'x_limits', [epoch_time_s_relative(1), epoch_time_s_relative(end)]); xlim(x_lims);
    y_lims = grabACHAnalysisTools.get_plot_param(plot_params, 'y_limits', []); if ~isempty(y_lims), ylim(y_lims); end
    grid on; box on;
    
    legend_str = {}; if num_trials > 1 && any(error_values > 0), legend_str{end+1} = char(error_metric_type); end; legend_str{end+1} = sprintf('Mean (%d trials)', num_trials);
    legend(legend_str, 'Location', 'best', 'Interpreter', 'none');
    
    if plot_params.save_plots
        grabACHAnalysisTools.save_plot_figure(fig_handle, string(plot_params.output_dir), sprintf('%s_average_trace_%s', plot_params.event_label, lower(char(error_metric_type))), 'png');
    end
end