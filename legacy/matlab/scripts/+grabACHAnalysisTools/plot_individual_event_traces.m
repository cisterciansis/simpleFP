function fig_handle = plot_individual_event_traces(epochs_signal, epoch_time_s_relative, plot_params)
% PLOT_INDIVIDUAL_EVENT_TRACES Plots individual signal traces for each epoch.
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
    if isempty(epochs_signal), disp('Info: No epochs for plot_individual_event_traces.'); return; end
    if size(epochs_signal, 1) ~= length(epoch_time_s_relative), error('grabACHAnalysisTools:plot_individual_event_traces:DimensionMismatch', 'Epoch signal rows must match time vector length.'); end

    num_epochs = size(epochs_signal, 2);
    y_label_str = plot_params.normalization_method_string;
    figure_name_str = sprintf('%s: Individual Traces (%s)', strrep(plot_params.event_label, '_', ' '), plot_params.normalization_method_string);
    
    fig_handle = figure('Name', figure_name_str, 'NumberTitle', 'off', 'Visible', 'on');
    fig_pos = grabACHAnalysisTools.get_plot_param(plot_params, 'figure_position', [100, 100, 800, 600]);
    set(fig_handle, 'Position', fig_pos);
    
    hold on;
    trace_colors = grabACHAnalysisTools.get_plot_colors(plot_params, num_epochs, 'trace_color', lines(min(num_epochs,7)), repmat([0.6 0.6 0.6],1,1));
    trace_line_width = grabACHAnalysisTools.get_plot_param(plot_params, 'line_width', 1);
    trace_alpha = grabACHAnalysisTools.get_plot_param(plot_params, 'trace_alpha', 0.5);

    for i = 1:num_epochs
        current_color_idx = mod(i-1, size(trace_colors,1)) + 1;
        plot(epoch_time_s_relative, epochs_signal(:, i), 'Color', [trace_colors(current_color_idx,:), trace_alpha], 'LineWidth', trace_line_width);
    end
    
    xline(0, 'k--', 'LineWidth', 1.5, 'Label', 'Event Onset', 'LabelHorizontalAlignment', 'center', 'LabelVerticalAlignment', 'bottom');
    hold off;
    
    xlabel(sprintf('Time from %s Onset (s)', strrep(plot_params.event_label, '_', ' ')));
    ylabel(y_label_str);
    title(figure_name_str, 'Interpreter', 'none');
    x_lims = grabACHAnalysisTools.get_plot_param(plot_params, 'x_limits', [epoch_time_s_relative(1), epoch_time_s_relative(end)]);
    xlim(x_lims);
    y_lims = grabACHAnalysisTools.get_plot_param(plot_params, 'y_limits', []);
    if ~isempty(y_lims), ylim(y_lims); end
    grid on; box on;
    
    if plot_params.save_plots
        grabACHAnalysisTools.save_plot_figure(fig_handle, string(plot_params.output_dir), sprintf('%s_individual_traces', plot_params.event_label), 'png');
    end
end